//! Event names and payloads sent to the webviews. All payloads are camelCase.
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::model::{AiStatus, CaptureResult, Counts, NextCard, Placement, RecordingStatus, SpeechStatus, SyncStatus};

pub const ITEMS_CHANGED: &str = "items-changed";
pub const COUNTS_CHANGED: &str = "counts-changed";
pub const PET_REACTION: &str = "pet-reaction";
pub const PET_FLAGS: &str = "pet-flags";
pub const POINTER_REGION: &str = "pointer-region";
pub const PET_VISIBILITY: &str = "pet-visibility";
pub const PET_PLACEMENT: &str = "pet-placement";
pub const AI_STATUS: &str = "ai-status";
pub const SPEECH_STATUS: &str = "speech-status";
pub const SETTINGS_CHANGED: &str = "settings-changed";
pub const CATEGORIES_CHANGED: &str = "categories-changed";
pub const TASKS_CHANGED: &str = "tasks-changed";
pub const HOTKEY: &str = "hotkey";
/// Home window only.
pub const NAVIGATE: &str = "navigate";
/// Pet window only: the outcome of a native drop on the pet.
pub const CAPTURE_RESULT: &str = "capture-result";
/// Pet window only: a proactive What's Next card.
pub const WHATS_NEXT: &str = "whats-next";
/// Pet window only: last week's diary notice.
pub const DIARY_READY: &str = "diary-ready";
/// Export jobs changed; listeners also refresh task lists (`TaskSummary.export`).
pub const EXPORTS_CHANGED: &str = "exports-changed";
/// A Notion/Asana connection finished (OAuth or token) or failed.
pub const INTEGRATION_CHANGED: &str = "integration-changed";
/// Agents screen data changed (a call, an activity row, a client, the HTTP server's state);
/// payload `{}`.
pub const AGENTS_CHANGED: &str = "agents-changed";
/// Sign-in, sign-out, the sync key or Delete cloud data changed the account; listeners re-read
/// `account_status`. `error` is set only when Google sign-in failed.
pub const ACCOUNT_CHANGED: &str = "account-changed";
/// The sync engine's state changed or a sync finished; payload `SyncStatus`.
pub const SYNC_STATUS: &str = "sync-status";
/// Meetings were created, changed or deleted.
pub const MEETINGS_CHANGED: &str = "meetings-changed";
/// The recorder's state changed; payload `RecordingStatus`.
pub const RECORDING_STATUS: &str = "recording-status";
/// Pet window only, every 100 ms while recording: track levels 0–1.
pub const RECORDING_LEVEL: &str = "recording-level";

/// At most one `celebrate` reaction per this gap.
const CELEBRATE_GAP: Duration = Duration::from_secs(10);
/// Agent calls send `agents-changed` at most once per this gap.
const AGENTS_GAP: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemsChanged {
    pub ids: Vec<String>,
}

/// `input` is a petMachine trigger name: `catch`, `camera`, `confused`, `idea`, `tilt`, `fidget`, …
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PetReaction {
    pub input: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<u8>,
}

impl PetReaction {
    pub fn new(input: &'static str) -> Self {
        PetReaction { input, category: None, variant: None }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PetFlags {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sleep: Option<bool>,
    /// The export worker is running jobs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub carrying: Option<bool>,
}

/// `region`: `"pet" | "panel" | "halo" | "zone"` or `None`; `target`: the hit panel's `id`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PointerRegion {
    pub region: Option<&'static str>,
    pub target: Option<String>,
    pub dragging: bool,
}

/// `reason`: `"fullscreen" | "user"` or `None`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PetVisibility {
    pub hidden: bool,
    pub reason: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsChanged {
    pub key: String,
}

/// `action`: `focusPet | sendSelection | captureScreen | quickNote | recordMeeting`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hotkey {
    pub action: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Navigate {
    pub route: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TasksChanged {
    pub ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingsChanged {
    pub ids: Vec<String>,
}

/// Levels 0–1 of the last 100 ms of each track (0 for a track that is off).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingLevel {
    pub mic: f32,
    pub system: f32,
}

/// Payload `{}`: listeners re-read `list_categories`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoriesChanged {}

/// `trigger`: `return | afterDone | afterMeeting`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatsNextEvent {
    pub card: NextCard,
    pub trigger: String,
}

/// `week`: this week's Monday (`YYYY-MM-DD`); the pet stores it in `diary.lastNoticeWeek` once shown.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiaryReady {
    pub week: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportsChanged {
    pub task_ids: Vec<String>,
}

/// `provider` is `None` only for an OAuth handoff whose state was never issued here.
/// `error`: `oauthExpired | oauthDenied | <AppError code>`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationChanged {
    pub provider: Option<String>,
    pub ok: bool,
    pub error: Option<String>,
}

/// Payload `{}`: listeners re-read `list_mcp_clients`, `list_agent_activity` and `mcp_status`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsChanged {}

/// `error`: `oauthExpired | otherAccount | <AppError code>` after a failed Google sign-in.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountChanged {
    pub error: Option<String>,
}

/// Emits to every webview; a failure to emit is logged, never fatal.
pub fn emit<S: Serialize + Clone>(app: &AppHandle, event: &str, payload: S) {
    if let Err(e) = app.emit(event, payload) {
        eprintln!("[events] emit {event} failed: {e}");
    }
}

pub fn items_changed(app: &AppHandle, ids: Vec<String>) {
    emit(app, ITEMS_CHANGED, ItemsChanged { ids });
}

pub fn tasks_changed(app: &AppHandle, ids: Vec<String>) {
    emit(app, TASKS_CHANGED, TasksChanged { ids });
}

pub fn meetings_changed(app: &AppHandle, ids: Vec<String>) {
    emit(app, MEETINGS_CHANGED, MeetingsChanged { ids });
}

pub fn exports_changed(app: &AppHandle, task_ids: Vec<String>) {
    emit(app, EXPORTS_CHANGED, ExportsChanged { task_ids });
}

pub fn integration_changed(app: &AppHandle, provider: Option<&str>, ok: bool, error: Option<&str>) {
    emit(
        app,
        INTEGRATION_CHANGED,
        IntegrationChanged { provider: provider.map(String::from), ok, error: error.map(String::from) },
    );
}

/// After an Agents command.
pub fn agents_changed(app: &AppHandle) {
    emit(app, AGENTS_CHANGED, AgentsChanged {});
}

pub fn account_changed(app: &AppHandle, error: Option<&str>) {
    emit(app, ACCOUNT_CHANGED, AccountChanged { error: error.map(String::from) });
}

pub fn sync_status(app: &AppHandle, s: SyncStatus) {
    emit(app, SYNC_STATUS, s);
}

/// When `agents-changed` goes out after a call or log write.
#[derive(Debug, PartialEq)]
enum Emit {
    Now,
    /// One emit at the end of the gap covers every call until then.
    After(Duration),
    /// That emit is already scheduled.
    Scheduled,
}

#[derive(Default)]
struct Throttle {
    last: Option<Instant>,
    scheduled: bool,
}

impl Throttle {
    fn emit(&mut self, now: Instant, gap: Duration) -> Emit {
        if self.scheduled {
            return Emit::Scheduled;
        }
        match self.last {
            Some(t) if now.duration_since(t) < gap => {
                self.scheduled = true;
                Emit::After(gap - now.duration_since(t))
            }
            _ => {
                self.last = Some(now);
                Emit::Now
            }
        }
    }

    fn emitted(&mut self, now: Instant) {
        self.last = Some(now);
        self.scheduled = false;
    }
}

/// After an agent call or activity log write: at most once per second, the last one never lost.
pub fn agents_changed_throttled(app: &AppHandle) {
    static STATE: Mutex<Throttle> = Mutex::new(Throttle { last: None, scheduled: false });
    let lock = || STATE.lock().unwrap_or_else(|e| e.into_inner());
    let next = lock().emit(Instant::now(), AGENTS_GAP);
    match next {
        Emit::Now => agents_changed(app),
        Emit::After(delay) => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(delay).await;
                lock().emitted(Instant::now());
                agents_changed(&app);
            });
        }
        Emit::Scheduled => {}
    }
}

pub fn counts_changed(app: &AppHandle, counts: Counts) {
    emit(app, COUNTS_CHANGED, counts);
}

pub fn pet_reaction(app: &AppHandle, r: PetReaction) {
    emit(app, PET_REACTION, r);
}

pub fn pet_flags(app: &AppHandle, f: PetFlags) {
    emit(app, PET_FLAGS, f);
}

pub fn pet_placement(app: &AppHandle, p: Placement) {
    emit(app, PET_PLACEMENT, p);
}

pub fn ai_status(app: &AppHandle, s: AiStatus) {
    emit(app, AI_STATUS, s);
}

pub fn speech_status(app: &AppHandle, s: SpeechStatus) {
    emit(app, SPEECH_STATUS, s);
}

pub fn recording_status(app: &AppHandle, s: RecordingStatus) {
    emit(app, RECORDING_STATUS, s);
}

pub fn categories_changed(app: &AppHandle) {
    emit(app, CATEGORIES_CHANGED, CategoriesChanged {});
}

/// Sent to the pet window only; it shows the notice or announcement for the drop.
pub fn capture_result(app: &AppHandle, r: CaptureResult) {
    if let Err(e) = app.emit_to(crate::windows::PET, CAPTURE_RESULT, r) {
        eprintln!("[events] emit {CAPTURE_RESULT} failed: {e}");
    }
}

/// Sent to the pet window only; it opens the card without taking focus.
pub fn whats_next(app: &AppHandle, ev: WhatsNextEvent) {
    if let Err(e) = app.emit_to(crate::windows::PET, WHATS_NEXT, ev) {
        eprintln!("[events] emit {WHATS_NEXT} failed: {e}");
    }
}

/// Sent to the pet window only; it shows last week's diary notice.
pub fn diary_ready(app: &AppHandle, ev: DiaryReady) {
    if let Err(e) = app.emit_to(crate::windows::PET, DIARY_READY, ev) {
        eprintln!("[events] emit {DIARY_READY} failed: {e}");
    }
}

/// Sent to the pet window only (the Recording HUD meters).
pub fn recording_level(app: &AppHandle, l: RecordingLevel) {
    if let Err(e) = app.emit_to(crate::windows::PET, RECORDING_LEVEL, l) {
        eprintln!("[events] emit {RECORDING_LEVEL} failed: {e}");
    }
}

/// `pet-reaction {input: "celebrate"}` when a task becomes done, at most once per 10 s.
pub fn celebrate(app: &AppHandle) {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let now = Instant::now();
    {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if !gap_passed(*last, now) {
            return;
        }
        *last = Some(now);
    }
    pet_reaction(app, PetReaction::new("celebrate"));
}

fn gap_passed(last: Option<Instant>, now: Instant) -> bool {
    last.is_none_or(|t| now.duration_since(t) >= CELEBRATE_GAP)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn celebrate_is_throttled_to_ten_seconds() {
        let t0 = Instant::now();
        assert!(gap_passed(None, t0));
        assert!(!gap_passed(Some(t0), t0 + Duration::from_millis(9_999)));
        assert!(gap_passed(Some(t0), t0 + CELEBRATE_GAP));
    }

    #[test]
    fn agents_changed_is_throttled_with_a_trailing_emit() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut t = Throttle::default();
        assert_eq!(t.emit(t0, AGENTS_GAP), Emit::Now);
        assert_eq!(t.emit(t0 + ms(300), AGENTS_GAP), Emit::After(ms(700)));
        assert_eq!(t.emit(t0 + ms(600), AGENTS_GAP), Emit::Scheduled);
        t.emitted(t0 + ms(1_000));
        assert_eq!(t.emit(t0 + ms(1_500), AGENTS_GAP), Emit::After(ms(500)));
        t.emitted(t0 + ms(2_000));
        assert_eq!(t.emit(t0 + ms(3_000), AGENTS_GAP), Emit::Now);
    }
}
