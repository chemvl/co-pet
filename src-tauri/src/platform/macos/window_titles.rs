//! Call detection signals besides the microphone: window titles of given processes (Zoom's
//! meeting window, a browser's Meet tab), the running apps' bundle ids, and whether a process with
//! a given name runs (Zoom's meeting helper). Also the `CGWindowListCopyWindowInfo` reader that
//! the full-screen check shares.
//!
//! Titles come from the window server when Screen Recording is granted, else from Accessibility.
//! They are matched in memory by the caller and never logged.
use std::ffi::{c_void, OsStr};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSRect, NSString};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

use super::permissions;
use crate::model::Rect;

/// `kCGWindowListOptionAll`: every window, including those on other Spaces and minimised ones.
const LIST_ALL: u32 = 0;
/// `AXError.success`.
const AX_SUCCESS: i32 = 0;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> *mut c_void;
    fn CGRectMakeWithDictionaryRepresentation(dict: *const c_void, rect: *mut NSRect) -> bool;
    static kCGWindowLayer: *const c_void;
    static kCGWindowOwnerPID: *const c_void;
    static kCGWindowBounds: *const c_void;
    static kCGWindowName: *const c_void;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> *mut c_void;
    fn AXUIElementCopyAttributeValue(
        element: *const c_void,
        attribute: *const c_void,
        value: *mut *mut c_void,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
}

/// One window from `CGWindowListCopyWindowInfo`.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowInfo {
    pub pid: i32,
    pub layer: i64,
    /// Read only when asked for (see [`window_list`]); `None` without Screen Recording.
    pub title: Option<String>,
    /// Global top-left points.
    pub bounds: Option<Rect>,
}

/// Where window titles can come from on this Mac right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleSource {
    ScreenRecording,
    Accessibility,
    None,
}

pub fn title_source() -> TitleSource {
    if permissions::screen_recording_granted() {
        TitleSource::ScreenRecording
    } else if permissions::accessibility_trusted() {
        TitleSource::Accessibility
    } else {
        TitleSource::None
    }
}

/// The non-empty titles of the windows of `pids`, as `(pid, title)`; empty when no title source
/// is available.
pub fn titles_for(pids: &[i32]) -> Vec<(i32, String)> {
    match title_source() {
        TitleSource::ScreenRecording => layer0_titles(&window_list(LIST_ALL, pids, true), pids),
        TitleSource::Accessibility => pids
            .iter()
            .flat_map(|&pid| ax_titles(pid).into_iter().map(move |t| (pid, t)))
            .collect(),
        TitleSource::None => Vec::new(),
    }
}

/// The non-empty titles of the normal (layer 0) windows of `pids`.
fn layer0_titles(windows: &[WindowInfo], pids: &[i32]) -> Vec<(i32, String)> {
    windows
        .iter()
        .filter(|w| w.layer == 0 && pids.contains(&w.pid))
        .filter_map(|w| w.title.as_ref().filter(|t| !t.is_empty()).map(|t| (w.pid, t.clone())))
        .collect()
}

/// The windows of `pids` that `CGWindowListCopyWindowInfo(options)` reports. Titles are read only
/// with `with_titles`, so callers that don't need them never touch them.
pub(crate) fn window_list(options: u32, pids: &[i32], with_titles: bool) -> Vec<WindowInfo> {
    // SAFETY: the function follows the Create rule (+1, released by `Retained`); it returns a
    // CFArray of CFDictionary, toll-free bridged to NSArray/NSDictionary.
    let raw = unsafe { CGWindowListCopyWindowInfo(options, 0) };
    let Some(list) = (unsafe { Retained::from_raw(raw.cast::<NSArray<NSDictionary<NSString, AnyObject>>>()) })
    else {
        return Vec::new();
    };
    // SAFETY: the keys are CFString constants, toll-free bridged to NSString.
    let (layer_key, pid_key, bounds_key, name_key) = unsafe {
        (
            &*(kCGWindowLayer as *const NSString),
            &*(kCGWindowOwnerPID as *const NSString),
            &*(kCGWindowBounds as *const NSString),
            &*(kCGWindowName as *const NSString),
        )
    };
    let number = |w: &NSDictionary<NSString, AnyObject>, key: &NSString| {
        w.objectForKey(key)
            .and_then(|v| v.downcast::<NSNumber>().ok())
            .map(|n| n.integerValue())
    };
    list.iter()
        .filter_map(|w| {
            let pid = i32::try_from(number(&w, pid_key)?).ok().filter(|pid| pids.contains(pid))?;
            let layer = number(&w, layer_key)? as i64;
            let bounds = w.objectForKey(bounds_key).and_then(|b| {
                let mut r = NSRect::ZERO;
                // SAFETY: `kCGWindowBounds` holds a CGRect dictionary representation.
                let ok = unsafe { CGRectMakeWithDictionaryRepresentation(Retained::as_ptr(&b).cast(), &mut r) };
                ok.then(|| Rect { x: r.origin.x, y: r.origin.y, w: r.size.width, h: r.size.height })
            });
            let title = with_titles
                .then(|| w.objectForKey(name_key).and_then(|v| v.downcast::<NSString>().ok()))
                .flatten()
                .map(|s| s.to_string());
            Some(WindowInfo { pid, layer, title, bounds })
        })
        .collect()
}

/// `kAXTitleAttribute` of each of `pid`'s `kAXWindowsAttribute` (needs Accessibility).
fn ax_titles(pid: i32) -> Vec<String> {
    let windows_attr = NSString::from_str("AXWindows");
    let title_attr = NSString::from_str("AXTitle");
    // SAFETY: Create rule (+1), released below; NULL for an invalid pid.
    let app = unsafe { AXUIElementCreateApplication(pid) };
    if app.is_null() {
        return Vec::new();
    }
    let windows = ax_attribute(app, &windows_attr).and_then(|v| v.downcast::<NSArray>().ok());
    // SAFETY: `app` came from a Create function and is released once.
    unsafe { CFRelease(app) };
    let Some(windows) = windows else { return Vec::new() };
    windows
        .iter()
        .filter_map(|w| ax_attribute(Retained::as_ptr(&w).cast(), &title_attr))
        .filter_map(|t| t.downcast::<NSString>().ok())
        .map(|t| t.to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

/// `AXUIElementCopyAttributeValue`, owned; `None` on any AX error.
fn ax_attribute(element: *const c_void, attribute: &NSString) -> Option<Retained<AnyObject>> {
    let mut value: *mut c_void = std::ptr::null_mut();
    // SAFETY: `element` is a live AXUIElement; the CFString attribute is toll-free bridged from
    // NSString; on success `value` holds a +1 CFType (Copy rule), bridged to an object.
    let err = unsafe { AXUIElementCopyAttributeValue(element, (attribute as *const NSString).cast(), &mut value) };
    if err != AX_SUCCESS {
        return None;
    }
    // SAFETY: +1 reference from the Copy rule, released by `Retained`.
    unsafe { Retained::from_raw(value.cast::<AnyObject>()) }
}

/// `(pid, bundle id)` of every running app that has a bundle id. Safe on any thread.
pub fn running_apps() -> Vec<(i32, String)> {
    NSWorkspace::sharedWorkspace()
        .runningApplications()
        .iter()
        .filter_map(|app| Some((app.processIdentifier(), app.bundleIdentifier()?.to_string())))
        .collect()
}

/// Whether a process whose name is exactly one of `names` runs.
pub fn process_running(names: &[&str]) -> bool {
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    sys.processes().values().any(|p| names.iter().any(|n| p.name() == OsStr::new(n)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(pid: i32, layer: i64, title: Option<&str>) -> WindowInfo {
        WindowInfo { pid, layer, title: title.map(str::to_string), bounds: None }
    }

    #[test]
    fn layer0_titles_keeps_normal_titled_windows_of_the_pids() {
        let windows = [
            window(10, 0, Some("Meet - Standup")),
            window(10, 25, Some("menu")),
            window(10, 0, Some("")),
            window(10, 0, None),
            window(20, 0, Some("Other app")),
            window(30, 0, Some("Zoom Meeting")),
        ];
        assert_eq!(
            layer0_titles(&windows, &[10, 30]),
            vec![(10, "Meet - Standup".to_string()), (30, "Zoom Meeting".to_string())]
        );
    }
}
