//! Event names and payloads sent to the webviews. All payloads are camelCase.
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::model::{AiStatus, CaptureResult, Counts, Placement};

pub const ITEMS_CHANGED: &str = "items-changed";
pub const COUNTS_CHANGED: &str = "counts-changed";
pub const PET_REACTION: &str = "pet-reaction";
pub const PET_FLAGS: &str = "pet-flags";
pub const POINTER_REGION: &str = "pointer-region";
pub const PET_VISIBILITY: &str = "pet-visibility";
pub const PET_PLACEMENT: &str = "pet-placement";
pub const AI_STATUS: &str = "ai-status";
pub const SETTINGS_CHANGED: &str = "settings-changed";
pub const HOTKEY: &str = "hotkey";
/// Home window only.
pub const NAVIGATE: &str = "navigate";
/// Pet window only: the outcome of a native drop on the pet.
pub const CAPTURE_RESULT: &str = "capture-result";

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

/// Emits to every webview; a failure to emit is logged, never fatal.
pub fn emit<S: Serialize + Clone>(app: &AppHandle, event: &str, payload: S) {
    if let Err(e) = app.emit(event, payload) {
        eprintln!("[events] emit {event} failed: {e}");
    }
}

pub fn items_changed(app: &AppHandle, ids: Vec<String>) {
    emit(app, ITEMS_CHANGED, ItemsChanged { ids });
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

/// Sent to the pet window only; it shows the notice or announcement for the drop.
pub fn capture_result(app: &AppHandle, r: CaptureResult) {
    if let Err(e) = app.emit_to(crate::windows::PET, CAPTURE_RESULT, r) {
        eprintln!("[events] emit {CAPTURE_RESULT} failed: {e}");
    }
}
