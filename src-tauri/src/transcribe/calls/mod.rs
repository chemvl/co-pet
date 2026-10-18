//! Zoom and Google Meet call detection. [`detect`] is the pure state machine; this is its runtime.
//! CoreAudio listeners wake one task when mic use changes; the task reads the signals on the
//! blocking pool, steps the [`Watch`] and acts on what it returns (pet-only events, the
//! auto-stop). While no Zoom or browser process uses the mic and no call is on, the task is
//! parked: no timer, no thread wake. With `transcribe.autoRecord = off` no listener exists.
pub mod detect;

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, Listener, Manager};
use tokio::sync::Notify;

use crate::db::{meetings, now_ms, settings, Db};
use crate::error::AppError;
use crate::events;
use crate::i18n::{self, tr, Lang};
use crate::model::{CallDetected, CallDetectionStatus, CallEnded, CallResumed};
use crate::platform::macos::fullscreen::Visibility;
use crate::platform::macos::mic_users::{self, MicUsers};
use crate::platform::macos::window_titles::{self, TitleSource};
use crate::transcribe::recorder::Recorder;
use detect::{
    Action, CallApp, Ctx, Mode, Snapshot, Watch, CHECK_EVERY_MS, ZOOM_BUNDLE, ZOOM_MEETING_PROCESSES,
};

const AUTO_RECORD: &str = "transcribe.autoRecord";
const AUTO_STOP: &str = "transcribe.autoStop";

/// Managed state (`app.state::<CallWatch>()`).
#[derive(Default)]
pub struct CallWatch {
    watch: Mutex<Watch>,
    /// Wakes the task: mic use changed, a call setting changed, or Keep recording.
    wake: Notify,
    /// The CoreAudio listeners; `None` while `transcribe.autoRecord` is `off`.
    listening: Mutex<Option<mic_users::Listening>>,
}

impl CallWatch {
    fn watch(&self) -> MutexGuard<'_, Watch> {
        lock(&self.watch)
    }

    /// `start_recording` for `call_id` started recording `meeting_id`: the recording now stops
    /// with the call when `auto_stop`. Returns the call's app and Meet name, or `None` when the
    /// call already ended or is unknown.
    fn link(&self, call_id: &str, meeting_id: &str, auto_stop: bool) -> Option<(CallApp, Option<String>)> {
        let linked = {
            let mut watch = self.watch();
            watch.link(call_id, meeting_id, auto_stop).then(|| watch.active()).flatten()
        };
        self.wake.notify_one();
        linked.map(|(_, app, name)| (app, name))
    }

    /// A recording started without a call: the call in progress, if any, is not offered.
    fn note_manual_recording(&self, meeting_id: &str) {
        self.watch().note_manual_recording(meeting_id);
    }

    /// Keep recording on a `call-ended` notice: the call's recording no longer stops with it.
    pub fn keep_recording(&self, call_id: &str) {
        self.watch().unlink(call_id);
        self.wake.notify_one();
    }

    /// Recording `meeting_id` ended (stopped by hand, at max length, or with its call): its call
    /// neither stops it again nor offers it again.
    pub fn unlink_meeting(&self, meeting_id: &str) {
        self.watch().unlink_meeting(meeting_id);
        self.wake.notify_one();
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Registers the mic listeners unless `transcribe.autoRecord` is `off`, follows
/// `transcribe.autoRecord` and `transcribe.autoStop`, and spawns the task. `CallWatch`, `Db` and
/// `Recorder` managed.
pub fn start(app: &AppHandle) {
    follow_mode(app);
    let handle = app.clone();
    app.listen_any(events::SETTINGS_CHANGED, move |event| {
        let key = serde_json::from_str::<Value>(event.payload()).ok();
        if matches!(key.as_ref().and_then(|v| v["key"].as_str()), Some(AUTO_RECORD | AUTO_STOP)) {
            follow_mode(&handle);
        }
    });
    tauri::async_runtime::spawn(run(app.clone()));
}

/// Adds the mic listeners when the mode is on and drops them when it is off, then wakes the task:
/// it re-reads the settings, and its first step catches a call already running.
fn follow_mode(app: &AppHandle) {
    let state = app.state::<CallWatch>();
    let on = mode(app) != Mode::Off;
    {
        let mut listening = lock(&state.listening);
        if !on {
            *listening = None;
        } else if listening.is_none() {
            let handle = app.clone();
            let wake = Box::new(move || handle.state::<CallWatch>().wake.notify_one());
            *listening = Some(mic_users::listen(wake));
        }
    }
    state.wake.notify_one();
}

async fn run(app: AppHandle) {
    let state = app.state::<CallWatch>();
    loop {
        let wait = {
            let watch = state.watch();
            watch.needs_checks().then(|| next_check(watch.next_deadline(), now_ms()))
        };
        match wait {
            Some(d) => {
                let _ = tokio::time::timeout(d, state.wake.notified()).await;
            }
            None => state.wake.notified().await,
        }
        let ctx = read_ctx(&app);
        if ctx.mode == Mode::Off {
            *state.watch() = Watch::default();
            continue;
        }
        let in_call = state.watch().active().is_some();
        let handle = app.clone();
        let snap = match tauri::async_runtime::spawn_blocking(move || collect_snapshot(&handle, in_call)).await {
            Ok(snap) => snap,
            Err(e) => {
                eprintln!("[calls] read signals: {e}");
                continue;
            }
        };
        let actions = state.watch().step(&snap, &ctx);
        for action in actions {
            apply(&app, action).await;
        }
    }
}

/// [`CHECK_EVERY_MS`], or less when a pending auto-stop is due sooner (zero once it is overdue).
fn next_check(deadline: Option<i64>, now: i64) -> Duration {
    let ms = deadline.map_or(CHECK_EVERY_MS, |d| (d - now).clamp(0, CHECK_EVERY_MS));
    Duration::from_millis(ms as u64)
}

/// Reads every signal [`detect::classify`] uses. Window titles and Zoom's meeting process matter
/// only while Zoom or a browser holds the mic (or the device flag stands for it) or a call is on,
/// so they are read only then, and titles only for Zoom and browser pids. Blocking.
fn collect_snapshot(app: &AppHandle, in_call: bool) -> Snapshot {
    // The listener can't tell a process-list change from an input toggle: follow the list on
    // every read, so a new process's input listener exists before its mic is read.
    if let Some(listening) = lock(&app.state::<CallWatch>().listening).as_mut() {
        listening.refresh_processes();
    }
    let co_pet_recording = app.state::<Recorder>().is_active();
    let (mic_users, device_running) = match mic_users::read() {
        // Unused by the classifier when processes are known.
        MicUsers::Processes(users) => (Some(users), false),
        MicUsers::DeviceOnly { running } => (None, running),
    };
    let mut snap = Snapshot {
        now: now_ms(),
        mic_users,
        device_running,
        apps: window_titles::running_apps(),
        zoom_meeting_process: false,
        titles: Vec::new(),
        co_pet_recording,
    };
    let sig = detect::classify(&snap);
    let mic = sig.zoom.as_ref().is_some_and(|z| z.mic) || sig.meet.as_ref().is_some_and(|m| m.mic);
    if !mic && !in_call {
        return snap;
    }
    let pids: Vec<i32> = snap
        .apps
        .iter()
        .filter(|(_, bundle)| bundle == ZOOM_BUNDLE || detect::is_browser(bundle))
        .map(|&(pid, _)| pid)
        .collect();
    if !pids.is_empty() {
        snap.titles = window_titles::titles_for(&pids);
    }
    snap.zoom_meeting_process = snap.apps.iter().any(|(_, bundle)| bundle == ZOOM_BUNDLE)
        && window_titles::process_running(ZOOM_MEETING_PROCESSES);
    snap
}

/// `transcribe.autoRecord`; `off` when unreadable.
fn mode(app: &AppHandle) -> Mode {
    match settings::read::<String>(app, AUTO_RECORD).as_deref() {
        Ok("ask") => Mode::Ask,
        Ok("auto") => Mode::Auto,
        Ok(_) => Mode::Off,
        Err(e) => {
            eprintln!("[calls] read {AUTO_RECORD}: {e}");
            Mode::Off
        }
    }
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Off => "off",
        Mode::Ask => "ask",
        Mode::Auto => "auto",
    }
}

/// `transcribe.autoStop`; `false` when unreadable.
fn auto_stop(app: &AppHandle) -> bool {
    settings::read::<bool>(app, AUTO_STOP).unwrap_or_else(|e| {
        eprintln!("[calls] read {AUTO_STOP}: {e}");
        false
    })
}

/// The live settings for a step. An unreadable setting never stops a recording or shows an offer.
/// Offers wait while the pet is paused, hidden by the user or hidden by a full-screen window.
fn read_ctx(app: &AppHandle) -> Ctx {
    let pet_shown = crate::next::pet_shown(&app.state::<Db>().conn(), now_ms()).unwrap_or_else(|e| {
        eprintln!("[calls] read pet visibility: {e}");
        false
    });
    let offers_allowed = pet_shown && !app.state::<Visibility>().current().hidden;
    Ctx { mode: mode(app), auto_stop: auto_stop(app), offers_allowed }
}

/// The title of a meeting recorded for a call: the app, plus the Meet name when known.
fn meeting_title(lang: Lang, app: CallApp, name: Option<&str>) -> String {
    match (app, name) {
        (CallApp::Zoom, _) => tr(lang, "meeting.titleZoom").to_owned(),
        (CallApp::Meet, Some(name)) => tr(lang, "meeting.titleMeet").replace("{name}", name),
        (CallApp::Meet, None) => tr(lang, "meeting.titleMeetPlain").to_owned(),
    }
}

/// `start_recording` started recording `meeting_id`; `call_id` is the `call-detected` call the
/// user chose to record. Linked to that call, the recording takes the call's title and, with
/// `transcribe.autoStop`, stops when the call ends. Started by hand, or for a call that already
/// ended or is unknown, it is a manual recording.
pub fn recording_started(app: &AppHandle, meeting_id: &str, call_id: Option<&str>) {
    let state = app.state::<CallWatch>();
    let Some((call_app, name)) = call_id.and_then(|id| state.link(id, meeting_id, auto_stop(app))) else {
        state.note_manual_recording(meeting_id);
        return;
    };
    let title = meeting_title(i18n::lang(app), call_app, name.as_deref());
    let saved = meetings::set_auto_title(&app.state::<Db>().conn(), meeting_id, &title);
    match saved {
        Ok(()) => meetings::notify_changed(app, vec![meeting_id.to_owned()]),
        Err(e) => eprintln!("[calls] meeting title: {e}"),
    }
}

async fn apply(app: &AppHandle, action: Action) {
    match action {
        Action::Offer { call_id, app: call_app, name } => {
            events::call_detected(app, CallDetected { call_id, app: call_app, name, mode: "ask" });
        }
        Action::AutoStart { call_id, app: call_app, name } => {
            events::call_detected(app, CallDetected { call_id, app: call_app, name, mode: "auto" });
        }
        Action::EndedStopping { call_id, app: call_app, stop_at } => {
            events::call_ended(app, CallEnded { call_id, app: call_app, stop_at_ms: Some(stop_at) });
        }
        Action::EndedManual { call_id, app: call_app } => {
            events::call_ended(app, CallEnded { call_id, app: call_app, stop_at_ms: None });
        }
        Action::Resumed { call_id } => events::call_resumed(app, CallResumed { call_id }),
        Action::Stop { meeting_id } => match app.state::<Recorder>().stop_for_call(app, &meeting_id).await {
            // The user already stopped it (or another session runs); a too-short one is discarded
            // and reported by the recorder itself.
            Ok(_) | Err(AppError::Code("notRecording" | "tooShort")) => {}
            Err(e) => eprintln!("[calls] stop at call end: {e}"),
        },
    }
}

/// `call_detection_status`: the mode, where the signals come from on this Mac, and the call in
/// progress. Reads CoreAudio and the permissions: call it off the async workers.
pub fn status(app: &AppHandle) -> CallDetectionStatus {
    let mode = mode(app);
    let active = match mode {
        Mode::Off => None,
        Mode::Ask | Mode::Auto => app.state::<CallWatch>().watch().active().map(|(call_id, call_app, name)| {
            CallDetected { call_id, app: call_app, name, mode: mode_name(mode) }
        }),
    };
    let titles = match window_titles::title_source() {
        TitleSource::ScreenRecording => "screenRecording",
        TitleSource::Accessibility => "accessibility",
        TitleSource::None => "none",
    };
    CallDetectionStatus {
        mode: mode_name(mode).into(),
        per_process_mic: matches!(mic_users::read(), MicUsers::Processes(_)),
        titles: titles.into(),
        active,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_check_waits_for_the_earlier_of_the_check_and_the_stop() {
        let every = Duration::from_millis(CHECK_EVERY_MS as u64);
        assert_eq!(next_check(None, 1_000), every);
        assert_eq!(next_check(Some(1_000 + CHECK_EVERY_MS * 2), 1_000), every);
        assert_eq!(next_check(Some(3_000), 1_000), Duration::from_millis(2_000));
        assert_eq!(next_check(Some(500), 1_000), Duration::ZERO, "overdue: step now");
    }
}
