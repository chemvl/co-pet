//! Event names and payloads sent to the webviews. All payloads are camelCase.
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::model::{AiStatus, CaptureResult, Counts, NextCard, Placement};

pub const ITEMS_CHANGED: &str = "items-changed";
pub const COUNTS_CHANGED: &str = "counts-changed";
pub const PET_REACTION: &str = "pet-reaction";
pub const PET_FLAGS: &str = "pet-flags";
pub const POINTER_REGION: &str = "pointer-region";
pub const PET_VISIBILITY: &str = "pet-visibility";
pub const PET_PLACEMENT: &str = "pet-placement";
pub const AI_STATUS: &str = "ai-status";
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
/// Export jobs changed; listeners also refresh task lists (`TaskSummary.export`).
pub const EXPORTS_CHANGED: &str = "exports-changed";
/// A Notion/Asana connection finished (OAuth or token) or failed.
pub const INTEGRATION_CHANGED: &str = "integration-changed";

/// At most one `celebrate` reaction per this gap.
const CELEBRATE_GAP: Duration = Duration::from_secs(10);

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

/// `region`: `"pet" | "panel" | "halo"` or `None`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PointerRegion {
    pub region: Option<&'static str>,
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

/// `action`: `focusPet | sendSelection | captureScreen | quickNote`.
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

/// Payload `{}`: listeners re-read `list_categories`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoriesChanged {}

/// `trigger`: `return | afterDone`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatsNextEvent {
    pub card: NextCard,
    pub trigger: String,
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
}
