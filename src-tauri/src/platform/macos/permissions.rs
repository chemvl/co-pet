//! Screen Recording, Accessibility, Microphone and Calendars: status checks, OS prompts and the
//! System Settings panes.
use std::ffi::c_void;
use std::sync::Mutex;
use std::time::Duration;

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_app_kit::NSWorkspace;
use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
use objc2_event_kit::{EKAuthorizationStatus, EKEntityType, EKEventStore};
use objc2_foundation::{NSDictionary, NSError, NSNumber, NSString, NSURL};
use tokio::sync::oneshot;

use crate::error::{AppError, AppResult};
use crate::model::PermissionsStatus;

/// How long `request_microphone` waits for the user to answer the OS prompt.
const MICROPHONE_PROMPT_TIMEOUT: Duration = Duration::from_secs(120);
/// How long `request_calendar` waits for the user to answer the OS prompt.
const CALENDAR_PROMPT_TIMEOUT: Duration = Duration::from_secs(120);

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    static kAXTrustedCheckOptionPrompt: *const c_void;
}

pub fn screen_recording_granted() -> bool {
    // SAFETY: no arguments; available since macOS 10.15.
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// Shows the OS prompt once and lists Co-pet under Screen Recording.
pub fn request_screen_recording() {
    // SAFETY: no arguments; available since macOS 10.15.
    unsafe { CGRequestScreenCaptureAccess() };
}

pub fn accessibility_trusted() -> bool {
    // SAFETY: no arguments.
    unsafe { AXIsProcessTrusted() }
}

/// Shows the OS prompt and lists Co-pet under Accessibility so the user only flips the switch.
pub fn request_accessibility() {
    // SAFETY: `kAXTrustedCheckOptionPrompt` is a CFString constant, toll-free bridged to NSString.
    let key: &NSString = unsafe { &*(kAXTrustedCheckOptionPrompt as *const NSString) };
    let yes = NSNumber::new_bool(true);
    let options = NSDictionary::<NSString, NSNumber>::from_slices(&[key], &[&*yes]);
    // SAFETY: an NSDictionary is toll-free bridged to the CFDictionary the function expects.
    unsafe { AXIsProcessTrustedWithOptions(&*options as *const _ as *const c_void) };
}

/// `granted` | `notDetermined` | `denied` (Denied or Restricted).
pub fn microphone_status() -> &'static str {
    // SAFETY: `AVMediaTypeAudio` is a framework constant, always present on macOS 13+.
    let audio = unsafe { AVMediaTypeAudio }.expect("AVMediaTypeAudio");
    // SAFETY: valid media type; callable from any thread.
    let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(audio) };
    microphone_label(status)
}

fn microphone_label(status: AVAuthorizationStatus) -> &'static str {
    match status {
        AVAuthorizationStatus::Authorized => "granted",
        AVAuthorizationStatus::NotDetermined => "notDetermined",
        _ => "denied",
    }
}

/// Shows the OS microphone prompt (only while `notDetermined`) and resolves to whether access is
/// granted; `false` when the user hasn't answered within two minutes.
pub async fn request_microphone() -> bool {
    let (tx, rx) = oneshot::channel::<bool>();
    {
        // Scoped: the block isn't `Send`, so it must be gone before the `await`.
        let tx = Mutex::new(Some(tx));
        let handler = RcBlock::new(move |granted: Bool| {
            if let Some(tx) = tx.lock().ok().and_then(|mut tx| tx.take()) {
                let _ = tx.send(granted.as_bool());
            }
        });
        // SAFETY: `AVMediaTypeAudio` is a framework constant, always present on macOS 13+.
        let audio = unsafe { AVMediaTypeAudio }.expect("AVMediaTypeAudio");
        // SAFETY: valid media type; AVFoundation copies the block and calls it once on any queue.
        unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(audio, &handler) };
    }
    matches!(tokio::time::timeout(MICROPHONE_PROMPT_TIMEOUT, rx).await, Ok(Ok(true)))
}

/// `granted` | `notDetermined` | `denied` (Denied, Restricted or WriteOnly).
pub fn calendar_status() -> &'static str {
    // SAFETY: class method, callable from any thread.
    let status = unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Event) };
    calendar_label(status)
}

fn calendar_label(status: EKAuthorizationStatus) -> &'static str {
    match status {
        EKAuthorizationStatus::FullAccess => "granted",
        EKAuthorizationStatus::NotDetermined => "notDetermined",
        _ => "denied",
    }
}

/// Shows the OS calendar prompt (only while `notDetermined`) and resolves to whether full access
/// is granted; `false` when the user hasn't answered within two minutes.
pub async fn request_calendar() -> bool {
    let (tx, rx) = oneshot::channel::<bool>();
    {
        // Scoped: the block and the store aren't `Send`, so they must be gone before the `await`.
        let tx = Mutex::new(Some(tx));
        // SAFETY: plain allocation of an event store.
        let store = unsafe { EKEventStore::new() };
        let keep = store.clone();
        let handler = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
            let _ = &keep; // the store must outlive the request
            if let Some(tx) = tx.lock().ok().and_then(|mut tx| tx.take()) {
                let _ = tx.send(granted.as_bool());
            }
        });
        if objc2::available!(macos = 14.0) {
            // SAFETY: valid block; EventKit copies it and calls it once on any queue.
            unsafe { store.requestFullAccessToEventsWithCompletion(RcBlock::as_ptr(&handler)) };
        } else {
            // SAFETY: valid block; EventKit copies it and calls it once on any queue.
            #[allow(deprecated)]
            unsafe {
                store.requestAccessToEntityType_completion(
                    EKEntityType::Event,
                    RcBlock::as_ptr(&handler),
                )
            };
        }
    }
    matches!(tokio::time::timeout(CALENDAR_PROMPT_TIMEOUT, rx).await, Ok(Ok(true)))
}

pub fn status() -> PermissionsStatus {
    PermissionsStatus {
        screen_recording: screen_recording_granted(),
        accessibility: accessibility_trusted(),
        microphone: microphone_status().to_owned(),
        calendar: calendar_status().to_owned(),
    }
}

/// Opens System Settings › Privacy & Security at the pane for `kind`.
pub fn open_privacy_pane(kind: &str) -> AppResult<()> {
    let anchor = match kind {
        "screenRecording" => "Privacy_ScreenCapture",
        "accessibility" => "Privacy_Accessibility",
        "microphone" => "Privacy_Microphone",
        "calendar" => "Privacy_Calendars",
        _ => return Err(AppError::Code("invalid")),
    };
    let link = format!("x-apple.systempreferences:com.apple.preference.security?{anchor}");
    let url = NSURL::URLWithString(&NSString::from_str(&link))
        .ok_or_else(|| AppError::msg(format!("bad settings url {link}")))?;
    if NSWorkspace::sharedWorkspace().openURL(&url) {
        Ok(())
    } else {
        Err(AppError::msg("couldn't open System Settings"))
    }
}

/// `request_permission`: prompt (which also registers Co-pet in the list), then open the pane.
/// The microphone and calendar prompts are the whole flow while undecided; once decided, only the
/// pane helps.
pub fn request(kind: &str) -> AppResult<()> {
    match kind {
        "screenRecording" => request_screen_recording(),
        "accessibility" => request_accessibility(),
        "microphone" if microphone_status() == "notDetermined" => {
            tauri::async_runtime::spawn(request_microphone());
            return Ok(());
        }
        "microphone" => {}
        "calendar" if calendar_status() == "notDetermined" => {
            tauri::async_runtime::spawn(request_calendar());
            return Ok(());
        }
        "calendar" => {}
        _ => return Err(AppError::Code("invalid")),
    }
    open_privacy_pane(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn microphone_label_maps_every_status() {
        assert_eq!(microphone_label(AVAuthorizationStatus::Authorized), "granted");
        assert_eq!(microphone_label(AVAuthorizationStatus::NotDetermined), "notDetermined");
        assert_eq!(microphone_label(AVAuthorizationStatus::Denied), "denied");
        assert_eq!(microphone_label(AVAuthorizationStatus::Restricted), "denied");
    }

    #[test]
    fn calendar_label_maps_every_status() {
        assert_eq!(calendar_label(EKAuthorizationStatus::FullAccess), "granted");
        assert_eq!(calendar_label(EKAuthorizationStatus::NotDetermined), "notDetermined");
        assert_eq!(calendar_label(EKAuthorizationStatus::Denied), "denied");
        assert_eq!(calendar_label(EKAuthorizationStatus::Restricted), "denied");
        assert_eq!(calendar_label(EKAuthorizationStatus::WriteOnly), "denied");
    }

    #[test]
    fn unknown_kinds_are_rejected() {
        assert!(matches!(request("camera"), Err(AppError::Code("invalid"))));
        assert!(matches!(open_privacy_pane("camera"), Err(AppError::Code("invalid"))));
    }
}
