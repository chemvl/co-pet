//! Call detection, pure: classifies one signal snapshot (who holds the mic, Zoom's meeting marker,
//! Zoom and browser window titles) and steps the call state machine. No OS access here; the
//! runtime reads the signals and acts on the returned [`Action`]s.
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use crate::platform::macos::mic_users::MicUser;

pub const ZOOM_BUNDLE: &str = "us.zoom.xos";
/// Process names that run only while Zoom is in a meeting.
pub const ZOOM_MEETING_PROCESSES: &[&str] = &["CptHost"];
/// Zoom window titles (prefix match) that mark a meeting.
pub const ZOOM_MEETING_TITLES: &[&str] = &["Zoom Meeting", "Zoom Webinar"];
pub const BROWSER_BUNDLE_PREFIXES: &[&str] = &[
    "com.google.Chrome",
    "com.microsoft.edgemac",
    "company.thebrowser.Browser",
    "com.brave.Browser",
    "org.mozilla.firefox",
    "com.vivaldi.Vivaldi",
    "com.operasoftware.Opera",
    "com.apple.Safari",
];
/// Safari captures the mic through this process; it counts only while Safari runs.
pub const SAFARI_MIC_BUNDLE: &str = "com.apple.WebKit.GPU";
const SAFARI_BUNDLE: &str = "com.apple.Safari";
/// A call is reported only after it has held this long.
pub const START_DEBOUNCE_MS: i64 = 8_000;
/// A call ends after all its signals have been gone this long.
pub const END_GRACE_MS: i64 = 20_000;
/// How often the runtime re-reads the signals while a candidate or call exists.
pub const CHECK_EVERY_MS: i64 = 5_000;

static MEET_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^Meet\s*[-–—]\s*(.+?)(\s*[-–—]\s*(Google Chrome|Microsoft Edge|Safari|Arc|Brave|Firefox|Vivaldi|Opera))?$",
    )
    .expect("valid Meet title pattern")
});

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CallApp {
    Zoom,
    Meet,
}

/// One reading of every signal the classifier uses.
pub struct Snapshot {
    pub now: i64,
    /// Processes with mic input running; None = older macOS (device flag only).
    pub mic_users: Option<Vec<MicUser>>,
    /// The default input runs for anyone, Co-pet included.
    pub device_running: bool,
    /// Running apps (pid, bundle id).
    pub apps: Vec<(i32, String)>,
    /// A Zoom meeting helper process ([`ZOOM_MEETING_PROCESSES`]) runs.
    pub zoom_meeting_process: bool,
    /// Titles of Zoom and browser windows (owner pid, title).
    pub titles: Vec<(i32, String)>,
    pub co_pet_recording: bool,
}

/// Per-app signals; `Some` while any of the app's signals holds (what keeps a call on).
#[derive(Debug, PartialEq, Eq)]
pub struct Signals {
    pub zoom: Option<ZoomSignal>,
    pub meet: Option<MeetSignal>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ZoomSignal {
    pub mic: bool,
    pub marker: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct MeetSignal {
    pub mic: bool,
    /// The meeting name from a Meet-titled browser window.
    pub name: Option<String>,
}

impl ZoomSignal {
    /// Zoom holds the mic and shows a meeting marker: a call starts.
    pub fn starts_call(&self) -> bool {
        self.mic && self.marker
    }
}

impl MeetSignal {
    /// A browser holds the mic and a Meet window exists: a call starts.
    pub fn starts_call(&self) -> bool {
        self.mic && self.name.is_some()
    }
}

pub fn classify(s: &Snapshot) -> Signals {
    let is_zoom: fn(&str) -> bool = |b| b == ZOOM_BUNDLE;
    let runs = |is_app: fn(&str) -> bool| s.apps.iter().any(|(_, b)| is_app(b));
    let safari_runs = s.apps.iter().any(|(_, b)| b == SAFARI_BUNDLE);
    // Older macOS: the device flag stands for the app's mic, except while Co-pet holds it.
    let device_mic = s.mic_users.is_none() && s.device_running && !s.co_pet_recording;
    let mic_held = |by: &dyn Fn(&str) -> bool| {
        s.mic_users.as_ref().is_some_and(|users| users.iter().any(|u| by(&u.bundle_id)))
    };

    let zoom_mic = mic_held(&is_zoom) || (device_mic && runs(is_zoom));
    let zoom_marker = s.zoom_meeting_process
        || app_titles(s, is_zoom).any(|t| ZOOM_MEETING_TITLES.iter().any(|m| t.starts_with(m)));
    let browser_mic = mic_held(&|b| is_browser(b) || (b == SAFARI_MIC_BUNDLE && safari_runs))
        || (device_mic && runs(is_browser));
    let name = app_titles(s, is_browser).find_map(meet_name);

    Signals {
        zoom: (zoom_mic || zoom_marker)
            .then_some(ZoomSignal { mic: zoom_mic, marker: zoom_marker }),
        meet: (browser_mic || name.is_some()).then_some(MeetSignal { mic: browser_mic, name }),
    }
}

pub(super) fn is_browser(bundle_id: &str) -> bool {
    BROWSER_BUNDLE_PREFIXES.iter().any(|p| bundle_id.starts_with(p))
}

/// Titles of the windows owned by a running app that `is_app` accepts.
fn app_titles(s: &Snapshot, is_app: fn(&str) -> bool) -> impl Iterator<Item = &str> {
    s.titles
        .iter()
        .filter(move |(pid, _)| s.apps.iter().any(|(p, b)| p == pid && is_app(b)))
        .map(|(_, title)| title.as_str())
}

/// The meeting name in a Meet window title (CGWindowList: "Meet – name"; Accessibility adds a
/// browser suffix: "Meet - name - Google Chrome").
pub fn meet_name(title: &str) -> Option<String> {
    let name = MEET_TITLE.captures(title)?.get(1)?.as_str().trim();
    (!name.is_empty()).then(|| name.to_owned())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Off,
    Ask,
    Auto,
}

pub struct Ctx {
    pub mode: Mode,
    pub auto_stop: bool,
    /// The pet may show an offer now: not paused and not hidden.
    pub offers_allowed: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Action {
    Offer { call_id: String, app: CallApp, name: Option<String> },
    AutoStart { call_id: String, app: CallApp, name: Option<String> },
    EndedStopping { call_id: String, app: CallApp, stop_at: i64 },
    EndedManual { call_id: String, app: CallApp },
    Resumed { call_id: String },
    Stop { meeting_id: String },
}

/// A recording started for a call.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Link {
    pub meeting_id: String,
    pub auto_stop: bool,
}

/// The call state machine: debounces a call's start, offers or auto-starts it once, and ends it
/// after [`END_GRACE_MS`] without any of its signals.
#[derive(Default)]
pub struct Watch {
    state: State,
    /// Zoom or a browser held the mic at the last step: a call may be about to start.
    mic_in_use: bool,
}

#[derive(Default)]
enum State {
    #[default]
    Idle,
    /// A call's start signals hold since `since`; reported once they have held for the debounce.
    Candidate { app: CallApp, since: i64 },
    InCall(Call),
    Ending(Ending),
}

/// A call none of whose signals holds since `since`.
struct Ending {
    call: Call,
    since: i64,
    /// `EndedStopping` was emitted for the pending auto-stop.
    announced: bool,
    /// The user chose Keep recording: the end passes silently.
    kept: bool,
}

struct Call {
    id: String,
    app: CallApp,
    name: Option<String>,
    linked: Option<Link>,
    /// Offered, auto-started, or covered by a recording that was running: never offered again.
    offered: bool,
}

impl Call {
    /// Any of the app's signals holds: the call is still on.
    fn stays(&self, sig: &Signals) -> bool {
        match self.app {
            CallApp::Zoom => sig.zoom.is_some(),
            CallApp::Meet => sig.meet.is_some(),
        }
    }

    /// The link to stop when the call ends: the recording asked for auto-stop and the setting is
    /// still on (turning it off mid-call cancels it; turning it on doesn't upgrade a link).
    fn auto_stop_link(&self, ctx: &Ctx) -> Option<&Link> {
        self.linked.as_ref().filter(|l| l.auto_stop && ctx.auto_stop)
    }

    /// Offers or auto-starts the call once; an offer the pet can't show now waits for a later step.
    fn offer(&mut self, s: &Snapshot, ctx: &Ctx, actions: &mut Vec<Action>) {
        if self.offered {
            return;
        }
        if s.co_pet_recording {
            self.offered = true;
            return;
        }
        let (call_id, app, name) = (self.id.clone(), self.app, self.name.clone());
        match ctx.mode {
            Mode::Auto => actions.push(Action::AutoStart { call_id, app, name }),
            Mode::Ask if ctx.offers_allowed => actions.push(Action::Offer { call_id, app, name }),
            Mode::Ask | Mode::Off => return,
        }
        self.offered = true;
    }
}

/// The app whose call start signals hold (Zoom first), with the Meet name.
fn detected(sig: &Signals) -> Option<(CallApp, Option<String>)> {
    if sig.zoom.as_ref().is_some_and(ZoomSignal::starts_call) {
        return Some((CallApp::Zoom, None));
    }
    sig.meet.as_ref().filter(|m| m.starts_call()).map(|m| (CallApp::Meet, m.name.clone()))
}

impl Watch {
    pub fn step(&mut self, s: &Snapshot, ctx: &Ctx) -> Vec<Action> {
        if ctx.mode == Mode::Off {
            *self = Self::default();
            return Vec::new();
        }
        let sig = classify(s);
        self.mic_in_use = sig.zoom.as_ref().is_some_and(|z| z.mic)
            || sig.meet.as_ref().is_some_and(|m| m.mic);
        let mut actions = Vec::new();
        self.state = match std::mem::take(&mut self.state) {
            State::Idle => match detected(&sig) {
                Some((app, _)) => State::Candidate { app, since: s.now },
                None => State::Idle,
            },
            State::Candidate { app, since } => match detected(&sig) {
                Some((seen, name)) if seen == app && s.now - since >= START_DEBOUNCE_MS => {
                    let id = match app {
                        CallApp::Zoom => format!("zoom:{since}"),
                        CallApp::Meet => format!("meet:{}", name.as_deref().unwrap_or_default()),
                    };
                    let mut call = Call { id, app, name, linked: None, offered: false };
                    call.offer(s, ctx, &mut actions);
                    State::InCall(call)
                }
                Some((seen, _)) if seen == app => State::Candidate { app, since },
                Some((seen, _)) => State::Candidate { app: seen, since: s.now },
                None => State::Idle,
            },
            State::InCall(mut call) if call.stays(&sig) => {
                call.offer(s, ctx, &mut actions);
                State::InCall(call)
            }
            State::InCall(call) => Ending { call, since: s.now, announced: false, kept: false }
                .step(s, &sig, ctx, &mut actions),
            State::Ending(ending) => ending.step(s, &sig, ctx, &mut actions),
        };
        actions
    }

    /// `start_recording` succeeded with this call id.
    pub fn link(&mut self, call_id: &str, meeting_id: &str, auto_stop: bool) -> bool {
        let Some(call) = self.call_mut().filter(|c| c.id == call_id) else {
            return false;
        };
        call.linked = Some(Link { meeting_id: meeting_id.to_owned(), auto_stop });
        call.offered = true;
        true
    }

    /// Any recording now running for which no link exists (manual).
    pub fn note_manual_recording(&mut self, meeting_id: &str) {
        if let Some(call) = self.call_mut() {
            if call.linked.as_ref().is_none_or(|l| l.meeting_id != meeting_id) {
                call.offered = true;
            }
        }
    }

    /// User pressed Keep recording.
    pub fn unlink(&mut self, call_id: &str) {
        match &mut self.state {
            State::InCall(call) if call.id == call_id => call.linked = None,
            State::Ending(Ending { call, kept, .. }) if call.id == call_id => {
                call.linked = None;
                *kept = true;
            }
            _ => {}
        }
    }

    /// The user stopped this recording by hand: drop any link to it (no auto-stop, no re-offer
    /// for that call).
    pub fn unlink_meeting(&mut self, meeting_id: &str) {
        if let Some(call) = self.call_mut() {
            if call.linked.as_ref().is_some_and(|l| l.meeting_id == meeting_id) {
                call.linked = None;
                call.offered = true;
            }
        }
    }

    /// Whether the runtime must keep re-reading signals.
    pub fn needs_checks(&self) -> bool {
        self.mic_in_use || !matches!(self.state, State::Idle)
    }

    /// When a pending auto-stop is due: the runtime must step by then. It can't see the current
    /// `Ctx`, so the step at the deadline decides whether `Stop` is still emitted.
    pub fn next_deadline(&self) -> Option<i64> {
        match &self.state {
            State::Ending(Ending { call, since, .. })
                if call.linked.as_ref().is_some_and(|l| l.auto_stop) =>
            {
                Some(since + END_GRACE_MS)
            }
            _ => None,
        }
    }

    /// The call in progress (call id, app, name), including one whose end is pending.
    pub fn active(&self) -> Option<(String, CallApp, Option<String>)> {
        match &self.state {
            State::InCall(call) | State::Ending(Ending { call, .. }) => {
                Some((call.id.clone(), call.app, call.name.clone()))
            }
            State::Idle | State::Candidate { .. } => None,
        }
    }

    fn call_mut(&mut self) -> Option<&mut Call> {
        match &mut self.state {
            State::InCall(call) | State::Ending(Ending { call, .. }) => Some(call),
            State::Idle | State::Candidate { .. } => None,
        }
    }
}

impl Ending {
    /// A linked auto-stop recording is announced at once and stopped at `since + END_GRACE_MS`;
    /// any other call ends silently then, asking about a Co-pet recording that still runs.
    /// Signals coming back resume the call.
    fn step(mut self, s: &Snapshot, sig: &Signals, ctx: &Ctx, actions: &mut Vec<Action>) -> State {
        let call = &mut self.call;
        if call.stays(sig) {
            if self.announced && call.linked.is_some() {
                actions.push(Action::Resumed { call_id: call.id.clone() });
            }
            call.offer(s, ctx, actions);
            return State::InCall(self.call);
        }
        let stop_at = self.since + END_GRACE_MS;
        if let Some(link) = call.auto_stop_link(ctx) {
            if !self.announced {
                let call_id = call.id.clone();
                actions.push(Action::EndedStopping { call_id, app: call.app, stop_at });
                self.announced = true;
            } else if s.now >= stop_at {
                actions.push(Action::Stop { meeting_id: link.meeting_id.clone() });
                return State::Idle;
            }
        } else if s.now >= stop_at {
            if !self.kept && s.co_pet_recording {
                actions.push(Action::EndedManual { call_id: call.id.clone(), app: call.app });
            }
            return State::Idle;
        }
        State::Ending(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZOOM_PID: i32 = 10;
    const CHROME_PID: i32 = 20;
    const CHROME_HELPER_PID: i32 = 21;
    const SAFARI_PID: i32 = 30;
    const WEBKIT_GPU_PID: i32 = 31;

    fn user(pid: i32, bundle_id: &str) -> MicUser {
        MicUser { pid, bundle_id: bundle_id.into() }
    }

    fn quiet(now: i64) -> Snapshot {
        Snapshot {
            now,
            mic_users: Some(vec![]),
            device_running: false,
            apps: vec![],
            zoom_meeting_process: false,
            titles: vec![],
            co_pet_recording: false,
        }
    }

    fn zoom_call(now: i64) -> Snapshot {
        Snapshot {
            mic_users: Some(vec![user(ZOOM_PID, ZOOM_BUNDLE)]),
            device_running: true,
            apps: vec![(ZOOM_PID, ZOOM_BUNDLE.into())],
            zoom_meeting_process: true,
            ..quiet(now)
        }
    }

    fn chrome() -> Vec<(i32, String)> {
        vec![(CHROME_PID, "com.google.Chrome".into())]
    }

    fn meet_call(now: i64) -> Snapshot {
        Snapshot {
            mic_users: Some(vec![user(CHROME_HELPER_PID, "com.google.Chrome.helper")]),
            device_running: true,
            apps: chrome(),
            titles: vec![(CHROME_PID, "Meet – abc-defg-hij".into())],
            ..quiet(now)
        }
    }

    fn recording(mut s: Snapshot) -> Snapshot {
        s.co_pet_recording = true;
        s
    }

    fn ctx(mode: Mode) -> Ctx {
        Ctx { mode, auto_stop: true, offers_allowed: true }
    }

    fn zoom_in_call(s: &Snapshot) -> bool {
        classify(s).zoom.is_some_and(|z| z.starts_call())
    }

    fn meet_in_call(s: &Snapshot) -> Option<String> {
        classify(s).meet.filter(MeetSignal::starts_call).and_then(|m| m.name)
    }

    /// Runs a Meet call through the debounce (offered at t=8 s); returns its call id.
    fn meet_offered(w: &mut Watch) -> String {
        assert_eq!(w.step(&meet_call(0), &ctx(Mode::Ask)), vec![]);
        let actions = w.step(&meet_call(8_000), &ctx(Mode::Ask));
        let [Action::Offer { call_id, .. }] = actions.as_slice() else {
            panic!("expected one offer, got {actions:?}");
        };
        call_id.clone()
    }

    /// A Meet call offered and linked to recording `m1`; returns its call id.
    fn meet_linked(w: &mut Watch, auto_stop: bool) -> String {
        let id = meet_offered(w);
        assert!(w.link(&id, "m1", auto_stop));
        id
    }

    // Zoom classification

    #[test]
    fn zoom_needs_a_meeting_marker() {
        let s = Snapshot { zoom_meeting_process: false, ..zoom_call(0) };
        assert!(!zoom_in_call(&s));
        let mut w = Watch::default();
        assert_eq!(w.step(&s, &ctx(Mode::Ask)), vec![]);
        assert_eq!(w.step(&Snapshot { now: 8_000, ..s }, &ctx(Mode::Ask)), vec![]);
        assert_eq!(w.active(), None);
    }

    #[test]
    fn zoom_in_meeting_with_helper() {
        assert!(zoom_in_call(&zoom_call(0)));
    }

    #[test]
    fn zoom_in_meeting_with_title_only() {
        let s = Snapshot {
            zoom_meeting_process: false,
            titles: vec![(ZOOM_PID, "Zoom Meeting".into())],
            ..zoom_call(0)
        };
        assert!(zoom_in_call(&s));
        let webinar = Snapshot { titles: vec![(ZOOM_PID, "Zoom Webinar 40-Minutes".into())], ..s };
        assert!(zoom_in_call(&webinar));
    }

    // Meet classification

    #[test]
    fn meet_needs_mic_and_title() {
        assert_eq!(meet_in_call(&meet_call(0)), Some("abc-defg-hij".into()));
        let no_mic = Snapshot { mic_users: Some(vec![]), ..meet_call(0) };
        assert_eq!(meet_in_call(&no_mic), None);
        let youtube = Snapshot { titles: vec![(CHROME_PID, "YouTube".into())], ..meet_call(0) };
        assert_eq!(meet_in_call(&youtube), None);
    }

    #[test]
    fn meet_name_parses_both_title_sources() {
        assert_eq!(meet_name("Meet – Weekly sync"), Some("Weekly sync".into()));
        assert_eq!(meet_name("Meet - abc-defg-hij - Google Chrome"), Some("abc-defg-hij".into()));
        assert_eq!(meet_name("Meeting notes – Google Docs"), None);
    }

    // Mic attribution

    #[test]
    fn own_and_system_processes_are_ignored() {
        let s = Snapshot {
            mic_users: Some(vec![user(40, "com.apple.replayd"), user(41, "com.example.unknown")]),
            device_running: true,
            apps: vec![(ZOOM_PID, ZOOM_BUNDLE.into()), (CHROME_PID, "com.google.Chrome".into())],
            zoom_meeting_process: true,
            titles: vec![(CHROME_PID, "Meet – abc-defg-hij".into())],
            ..quiet(0)
        };
        assert!(!zoom_in_call(&s));
        assert_eq!(meet_in_call(&s), None);
    }

    #[test]
    fn safari_counts_only_while_safari_runs() {
        let safari = Snapshot {
            mic_users: Some(vec![user(WEBKIT_GPU_PID, SAFARI_MIC_BUNDLE)]),
            device_running: true,
            apps: vec![(SAFARI_PID, SAFARI_BUNDLE.into())],
            titles: vec![(SAFARI_PID, "Meet – abc-defg-hij".into())],
            ..quiet(0)
        };
        assert_eq!(meet_in_call(&safari), Some("abc-defg-hij".into()));
        let not_running = Snapshot { apps: vec![], ..safari };
        assert_eq!(classify(&not_running).meet, None);
        let agent_only = Snapshot {
            apps: vec![(SAFARI_PID + 100, "com.apple.Safari.SafeBrowsing.Service".into())],
            ..not_running
        };
        assert_eq!(classify(&agent_only).meet, None);
    }

    // Starting a call

    #[test]
    fn debounce_before_reporting() {
        let mut w = Watch::default();
        assert_eq!(w.step(&meet_call(0), &ctx(Mode::Ask)), vec![]);
        assert_eq!(w.step(&meet_call(5_000), &ctx(Mode::Ask)), vec![]);
        assert_eq!(
            w.step(&meet_call(8_000), &ctx(Mode::Ask)),
            vec![Action::Offer {
                call_id: "meet:abc-defg-hij".into(),
                app: CallApp::Meet,
                name: Some("abc-defg-hij".into()),
            }]
        );
    }

    #[test]
    fn one_offer_per_call() {
        let mut w = Watch::default();
        meet_offered(&mut w);
        assert_eq!(w.step(&meet_call(13_000), &ctx(Mode::Ask)), vec![]);
        assert_eq!(w.step(&meet_call(18_000), &ctx(Mode::Ask)), vec![]);
    }

    #[test]
    fn auto_mode_starts() {
        let mut w = Watch::default();
        assert_eq!(w.step(&zoom_call(0), &ctx(Mode::Auto)), vec![]);
        assert_eq!(
            w.step(&zoom_call(8_000), &ctx(Mode::Auto)),
            vec![Action::AutoStart { call_id: "zoom:0".into(), app: CallApp::Zoom, name: None }]
        );
        assert_eq!(w.step(&zoom_call(13_000), &ctx(Mode::Auto)), vec![]);
    }

    #[test]
    fn off_mode_does_nothing() {
        let mut w = Watch::default();
        for now in [0, 8_000, 13_000] {
            assert_eq!(w.step(&zoom_call(now), &ctx(Mode::Off)), vec![]);
            assert!(!w.needs_checks());
        }
        assert_eq!(w.active(), None);
    }

    #[test]
    fn paused_pet_suppresses_offers_not_auto() {
        let paused = |mode| Ctx { mode, auto_stop: true, offers_allowed: false };
        let mut ask = Watch::default();
        assert_eq!(ask.step(&meet_call(0), &paused(Mode::Ask)), vec![]);
        assert_eq!(ask.step(&meet_call(8_000), &paused(Mode::Ask)), vec![]);
        assert_eq!(ask.step(&meet_call(13_000), &paused(Mode::Ask)), vec![]);

        let mut auto = Watch::default();
        assert_eq!(auto.step(&meet_call(0), &paused(Mode::Auto)), vec![]);
        assert!(matches!(
            auto.step(&meet_call(8_000), &paused(Mode::Auto)).as_slice(),
            [Action::AutoStart { app: CallApp::Meet, .. }]
        ));
    }

    #[test]
    fn suppressed_offer_is_shown_after_unpausing() {
        let paused = Ctx { mode: Mode::Ask, auto_stop: true, offers_allowed: false };
        let mut w = Watch::default();
        assert_eq!(w.step(&meet_call(0), &paused), vec![]);
        assert_eq!(w.step(&meet_call(8_000), &paused), vec![]);
        // Still recording by hand when the pet comes back: no offer for it.
        let mut busy = Watch::default();
        busy.step(&meet_call(0), &paused);
        busy.step(&meet_call(8_000), &paused);
        assert_eq!(busy.step(&recording(meet_call(13_000)), &ctx(Mode::Ask)), vec![]);

        assert!(matches!(
            w.step(&meet_call(13_000), &ctx(Mode::Ask)).as_slice(),
            [Action::Offer { app: CallApp::Meet, .. }]
        ));
        assert_eq!(w.step(&meet_call(18_000), &ctx(Mode::Ask)), vec![]);
    }

    #[test]
    fn no_offer_while_recording() {
        let mut w = Watch::default();
        assert_eq!(w.step(&recording(meet_call(0)), &ctx(Mode::Auto)), vec![]);
        assert_eq!(w.step(&recording(meet_call(8_000)), &ctx(Mode::Auto)), vec![]);
        assert_eq!(w.step(&recording(meet_call(13_000)), &ctx(Mode::Auto)), vec![]);
        let (call_id, app, _) = w.active().expect("the call is tracked");
        assert_eq!(w.step(&recording(quiet(18_000)), &ctx(Mode::Auto)), vec![]);
        assert_eq!(
            w.step(&recording(quiet(38_000)), &ctx(Mode::Auto)),
            vec![Action::EndedManual { call_id, app }]
        );
    }

    // Staying in call

    #[test]
    fn tab_switch_keeps_call() {
        let mut w = Watch::default();
        meet_linked(&mut w, true);
        for now in [13_000, 28_000, 43_000] {
            let other_tab =
                Snapshot { titles: vec![(CHROME_PID, "Docs".into())], ..recording(meet_call(now)) };
            assert_eq!(w.step(&other_tab, &ctx(Mode::Ask)), vec![]);
        }
        assert!(w.active().is_some());
        assert_eq!(w.next_deadline(), None);
    }

    #[test]
    fn mute_keeps_call() {
        let mut w = Watch::default();
        meet_linked(&mut w, true);
        for now in [13_000, 28_000, 43_000] {
            let muted = Snapshot { mic_users: Some(vec![]), ..recording(meet_call(now)) };
            assert_eq!(w.step(&muted, &ctx(Mode::Ask)), vec![]);
        }
        assert!(w.active().is_some());
    }

    #[test]
    fn reconnect_within_grace_resumes() {
        let mut w = Watch::default();
        let id = meet_linked(&mut w, true);
        assert!(matches!(
            w.step(&recording(quiet(13_000)), &ctx(Mode::Ask)).as_slice(),
            [Action::EndedStopping { stop_at: 33_000, .. }]
        ));
        assert_eq!(w.step(&recording(quiet(18_000)), &ctx(Mode::Ask)), vec![]);
        assert_eq!(
            w.step(&recording(meet_call(23_000)), &ctx(Mode::Ask)),
            vec![Action::Resumed { call_id: id.clone() }]
        );
        assert_eq!(w.step(&recording(meet_call(33_000)), &ctx(Mode::Ask)), vec![]);
        assert_eq!(w.active().map(|a| a.0), Some(id));
        assert_eq!(w.next_deadline(), None);
    }

    #[test]
    fn reconnect_without_auto_stop_is_silent() {
        let mut w = Watch::default();
        meet_linked(&mut w, false);
        assert_eq!(w.step(&recording(quiet(13_000)), &ctx(Mode::Ask)), vec![]);
        assert_eq!(w.step(&recording(meet_call(23_000)), &ctx(Mode::Ask)), vec![]);
        assert_eq!(w.step(&recording(quiet(43_000)), &ctx(Mode::Ask)), vec![]);
        assert!(w.active().is_some());
    }

    // Ending a call

    #[test]
    fn end_after_grace_stops_linked() {
        let mut w = Watch::default();
        let id = meet_linked(&mut w, true);
        assert_eq!(
            w.step(&recording(quiet(13_000)), &ctx(Mode::Ask)),
            vec![Action::EndedStopping { call_id: id, app: CallApp::Meet, stop_at: 33_000 }]
        );
        assert_eq!(w.next_deadline(), Some(33_000));
        assert_eq!(w.step(&recording(quiet(18_000)), &ctx(Mode::Ask)), vec![]);
        assert_eq!(w.step(&recording(quiet(32_999)), &ctx(Mode::Ask)), vec![]);
        assert_eq!(
            w.step(&recording(quiet(33_000)), &ctx(Mode::Ask)),
            vec![Action::Stop { meeting_id: "m1".into() }]
        );
        assert_eq!(w.active(), None);
        assert_eq!(w.next_deadline(), None);
        assert!(!w.needs_checks());
    }

    #[test]
    fn auto_stop_off_asks_instead() {
        let mut w = Watch::default();
        let id = meet_linked(&mut w, false);
        assert_eq!(w.step(&recording(quiet(13_000)), &ctx(Mode::Ask)), vec![]);
        assert_eq!(w.next_deadline(), None);
        assert_eq!(w.step(&recording(quiet(28_000)), &ctx(Mode::Ask)), vec![]);
        assert_eq!(
            w.step(&recording(quiet(33_000)), &ctx(Mode::Ask)),
            vec![Action::EndedManual { call_id: id, app: CallApp::Meet }]
        );
        assert_eq!(w.active(), None);
    }

    #[test]
    fn call_end_without_recording_is_silent() {
        let mut w = Watch::default();
        meet_offered(&mut w);
        assert_eq!(w.step(&quiet(13_000), &ctx(Mode::Ask)), vec![]);
        assert_eq!(w.step(&quiet(33_000), &ctx(Mode::Ask)), vec![]);
        assert_eq!(w.active(), None);
        assert!(!w.needs_checks());
    }

    #[test]
    fn manual_recording_is_never_stopped() {
        let mut w = Watch::default();
        let id = meet_offered(&mut w);
        w.note_manual_recording("m1");
        assert_eq!(w.step(&recording(meet_call(13_000)), &ctx(Mode::Ask)), vec![]);
        assert_eq!(w.step(&recording(quiet(18_000)), &ctx(Mode::Ask)), vec![]);
        assert_eq!(w.next_deadline(), None);
        assert_eq!(
            w.step(&recording(quiet(38_000)), &ctx(Mode::Ask)),
            vec![Action::EndedManual { call_id: id, app: CallApp::Meet }]
        );
    }

    #[test]
    fn keep_recording_unlinks() {
        let mut w = Watch::default();
        let id = meet_linked(&mut w, true);
        assert!(matches!(
            w.step(&recording(quiet(13_000)), &ctx(Mode::Ask)).as_slice(),
            [Action::EndedStopping { .. }]
        ));
        w.unlink(&id);
        assert_eq!(w.next_deadline(), None);
        assert_eq!(w.step(&recording(quiet(33_000)), &ctx(Mode::Ask)), vec![]);
        assert_eq!(w.active(), None);
    }

    #[test]
    fn auto_stop_turned_off_mid_call_never_stops() {
        let off = Ctx { mode: Mode::Ask, auto_stop: false, offers_allowed: true };
        // Turned off before the call ends: no countdown, the user is asked instead.
        let mut w = Watch::default();
        let id = meet_linked(&mut w, true);
        assert_eq!(w.step(&recording(quiet(13_000)), &off), vec![]);
        assert_eq!(
            w.step(&recording(quiet(33_000)), &off),
            vec![Action::EndedManual { call_id: id, app: CallApp::Meet }]
        );

        // Turned off during the countdown: no `Stop` at the deadline.
        let mut w = Watch::default();
        let id = meet_linked(&mut w, true);
        assert!(matches!(
            w.step(&recording(quiet(13_000)), &ctx(Mode::Ask)).as_slice(),
            [Action::EndedStopping { stop_at: 33_000, .. }]
        ));
        assert_eq!(w.next_deadline(), Some(33_000));
        assert_eq!(
            w.step(&recording(quiet(33_000)), &off),
            vec![Action::EndedManual { call_id: id, app: CallApp::Meet }]
        );
        assert_eq!(w.active(), None);

        // Turned on again does not upgrade a link made without auto-stop.
        let mut w = Watch::default();
        meet_linked(&mut w, false);
        assert_eq!(w.step(&recording(quiet(13_000)), &ctx(Mode::Ask)), vec![]);
        assert_eq!(w.next_deadline(), None);
    }

    #[test]
    fn stopping_by_hand_unlinks_without_reoffer() {
        let mut w = Watch::default();
        let id = meet_linked(&mut w, true);
        w.unlink_meeting("m1");
        assert_eq!(w.step(&meet_call(13_000), &ctx(Mode::Ask)), vec![]);
        assert_eq!(w.step(&quiet(18_000), &ctx(Mode::Ask)), vec![]);
        assert_eq!(w.next_deadline(), None);
        assert_eq!(w.step(&quiet(38_000), &ctx(Mode::Ask)), vec![]);
        assert!(!w.link(&id, "m2", true), "the call is over");
    }

    // Older OS

    #[test]
    fn device_only_fallback_uses_markers_while_recording() {
        let zoom = |now, marker, rec| Snapshot {
            now,
            mic_users: None,
            device_running: true,
            apps: vec![(ZOOM_PID, ZOOM_BUNDLE.into())],
            zoom_meeting_process: marker,
            titles: vec![],
            co_pet_recording: rec,
        };
        let mut w = Watch::default();
        assert_eq!(w.step(&zoom(0, true, false), &ctx(Mode::Ask)), vec![]);
        let actions = w.step(&zoom(8_000, true, false), &ctx(Mode::Ask));
        let [Action::Offer { call_id, .. }] = actions.as_slice() else {
            panic!("expected one offer, got {actions:?}");
        };
        assert!(w.link(call_id, "m1", true));
        // Co-pet holds the device: only the marker keeps the call on.
        assert_eq!(w.step(&zoom(13_000, true, true), &ctx(Mode::Ask)), vec![]);
        assert!(matches!(
            w.step(&zoom(18_000, false, true), &ctx(Mode::Ask)).as_slice(),
            [Action::EndedStopping { stop_at: 38_000, .. }]
        ));
        assert_eq!(
            w.step(&zoom(38_000, false, true), &ctx(Mode::Ask)),
            vec![Action::Stop { meeting_id: "m1".into() }]
        );
    }

    // Debounce identity

    #[test]
    fn candidate_switching_app_restarts_debounce() {
        let mut w = Watch::default();
        assert_eq!(w.step(&zoom_call(0), &ctx(Mode::Ask)), vec![]);
        assert!(w.needs_checks());
        assert_eq!(w.step(&meet_call(5_000), &ctx(Mode::Ask)), vec![]);
        assert_eq!(w.step(&meet_call(8_000), &ctx(Mode::Ask)), vec![]);
        assert!(matches!(
            w.step(&meet_call(13_000), &ctx(Mode::Ask)).as_slice(),
            [Action::Offer { app: CallApp::Meet, .. }]
        ));
    }
}
