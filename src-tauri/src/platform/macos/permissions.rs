//! Screen Recording and Accessibility: status checks, OS prompts and the System Settings panes.
use std::ffi::c_void;

use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

use crate::error::{AppError, AppResult};
use crate::model::PermissionsStatus;

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

pub fn status() -> PermissionsStatus {
    PermissionsStatus {
        screen_recording: screen_recording_granted(),
        accessibility: accessibility_trusted(),
    }
}

/// Opens System Settings › Privacy & Security at the pane for `kind`.
pub fn open_privacy_pane(kind: &str) -> AppResult<()> {
    let anchor = match kind {
        "screenRecording" => "Privacy_ScreenCapture",
        "accessibility" => "Privacy_Accessibility",
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
pub fn request(kind: &str) -> AppResult<()> {
    match kind {
        "screenRecording" => request_screen_recording(),
        "accessibility" => request_accessibility(),
        _ => return Err(AppError::Code("invalid")),
    }
    open_privacy_pane(kind)
}
