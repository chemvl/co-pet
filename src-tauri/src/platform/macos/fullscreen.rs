//! Hidden states (Step 10.3). The pet hides while the frontmost app has a full-screen window on the
//! pet's monitor, and while the user's "Hide for 1 hour" (`pet.hiddenUntil`) runs. Polled once a
//! second; `pet-visibility` is emitted on change and the pointer loop stops hit-testing while hidden.
//!
//! On a display with a camera notch a full-screen window starts below the notch
//! (`safeAreaInsets.top`), so it covers the frame minus that strip rather than the whole frame.
use std::ffi::c_void;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::MainThreadMarker;
use objc2_app_kit::NSScreen;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSRect, NSString};
use serde_json::Value;
use tauri::{AppHandle, Manager};

use super::{frontmost, on_main, pointer::PointerState};
use crate::db::{now_ms, settings};
use crate::events::{self, PetVisibility};
use crate::model::Rect;
use crate::windows;

const TICK: Duration = Duration::from_secs(1);
const HIDDEN_UNTIL: &str = "pet.hiddenUntil";
/// `kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements`.
const LIST_OPTIONS: u32 = (1 << 0) | (1 << 4);
/// Window bounds and screen frames are both in points; allow rounding.
const TOLERANCE: f64 = 1.0;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> *mut c_void;
    fn CGRectMakeWithDictionaryRepresentation(dict: *const c_void, rect: *mut NSRect) -> bool;
    static kCGWindowLayer: *const c_void;
    static kCGWindowOwnerPID: *const c_void;
    static kCGWindowBounds: *const c_void;
}

/// Managed: the last visibility sent to the pet (`None` until the first poll with a pet window).
#[derive(Default)]
pub struct Visibility(Mutex<Option<PetVisibility>>);

impl Visibility {
    fn lock(&self) -> MutexGuard<'_, Option<PetVisibility>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn current(&self) -> PetVisibility {
        self.lock().clone().unwrap_or(VISIBLE)
    }
}

const VISIBLE: PetVisibility = PetVisibility { hidden: false, reason: None };

pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(TICK);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let Some(frame) = windows::pet_screen_frame(&app) else { continue };
            let user = user_hidden(&app);
            // A user hide wins over full screen, so the window list isn't needed meanwhile.
            let full = user
                || match on_main(&app, move |mtm| frontmost_fullscreen_on(mtm, frame)).await {
                    Ok(full) => full,
                    Err(_) => break, // the event loop is gone: the app is exiting
                };
            let next = match (user, full) {
                (true, _) => PetVisibility { hidden: true, reason: Some("user") },
                (false, true) => PetVisibility { hidden: true, reason: Some("fullscreen") },
                (false, false) => VISIBLE,
            };
            update(&app, next);
        }
    });
}

/// "Show pet" and the focus hotkey: ends a user hide now instead of at the next poll. A
/// full-screen hide stays (the next poll re-evaluates it).
pub fn show_now(app: &AppHandle) {
    let until = settings::read::<Value>(app, HIDDEN_UNTIL).unwrap_or(Value::Null);
    if !until.is_null() {
        if let Err(e) = settings::set(app, HIDDEN_UNTIL, &Value::Null) {
            eprintln!("[visibility] clear {HIDDEN_UNTIL}: {e}");
        }
    }
    let user_hidden = app.state::<Visibility>().current().reason == Some("user");
    if user_hidden {
        update(app, VISIBLE);
    }
}

/// `now < pet.hiddenUntil`; a passed deadline is cleared.
fn user_hidden(app: &AppHandle) -> bool {
    let Ok(Some(until)) = settings::read::<Option<f64>>(app, HIDDEN_UNTIL) else { return false };
    if (now_ms() as f64) < until {
        return true;
    }
    if let Err(e) = settings::set(app, HIDDEN_UNTIL, &Value::Null) {
        eprintln!("[visibility] clear {HIDDEN_UNTIL}: {e}");
    }
    false
}

fn update(app: &AppHandle, next: PetVisibility) {
    let state = app.state::<Visibility>();
    let mut last = state.lock();
    if last.as_ref() == Some(&next) {
        return;
    }
    eprintln!("[visibility] hidden={} reason={:?}", next.hidden, next.reason);
    app.state::<PointerState>().set_hidden(next.hidden);
    events::emit(app, events::PET_VISIBILITY, next.clone());
    *last = Some(next);
}

/// Whether some on-screen, layer-0 window of the frontmost app covers `frame` (global top-left
/// points, the space `CGWindowListCopyWindowInfo` reports bounds in), or all of it below the notch.
fn frontmost_fullscreen_on(mtm: MainThreadMarker, frame: Rect) -> bool {
    let Some(front_pid) = frontmost::frontmost_pid() else { return false };
    if front_pid == std::process::id() as i32 {
        return false;
    }
    // SAFETY: the function follows the Create rule (+1, released by `Retained`); it returns a
    // CFArray of CFDictionary, toll-free bridged to NSArray/NSDictionary.
    let raw = unsafe { CGWindowListCopyWindowInfo(LIST_OPTIONS, 0) };
    let Some(list) = (unsafe { Retained::from_raw(raw.cast::<NSArray<NSDictionary<NSString, AnyObject>>>()) })
    else {
        return false;
    };
    // SAFETY: the keys are CFString constants, toll-free bridged to NSString.
    let (layer_key, pid_key, bounds_key) = unsafe {
        (
            &*(kCGWindowLayer as *const NSString),
            &*(kCGWindowOwnerPID as *const NSString),
            &*(kCGWindowBounds as *const NSString),
        )
    };
    let number = |w: &NSDictionary<NSString, AnyObject>, key: &NSString| {
        w.objectForKey(key)
            .and_then(|v| v.downcast::<NSNumber>().ok())
            .map(|n| n.integerValue())
    };
    let near = |a: f64, b: f64| (a - b).abs() <= TOLERANCE;
    let notch = notch_height(mtm, frame);
    list.iter().any(|w| {
        if number(&w, layer_key) != Some(0) || number(&w, pid_key) != Some(front_pid as isize) {
            return false;
        }
        let Some(bounds) = w.objectForKey(bounds_key) else { return false };
        let mut r = NSRect::ZERO;
        // SAFETY: `kCGWindowBounds` holds a CGRect dictionary representation.
        let ok = unsafe { CGRectMakeWithDictionaryRepresentation(Retained::as_ptr(&bounds).cast(), &mut r) };
        let top = r.origin.y - frame.y;
        ok && near(r.origin.x, frame.x)
            && near(r.size.width, frame.w)
            && near(r.origin.y + r.size.height, frame.y + frame.h)
            && (near(top, 0.0) || (notch > 0.0 && near(top, notch)))
    })
}

/// `safeAreaInsets.top` of the display whose frame is `frame` (0 without a notch). The x origin
/// and the size are the same in AppKit and top-left coordinates, so they identify the display.
fn notch_height(mtm: MainThreadMarker, frame: Rect) -> f64 {
    NSScreen::screens(mtm)
        .iter()
        .find(|s| {
            let f = s.frame();
            (f.origin.x - frame.x).abs() <= TOLERANCE
                && (f.size.width - frame.w).abs() <= TOLERANCE
                && (f.size.height - frame.h).abs() <= TOLERANCE
        })
        .map_or(0.0, |s| s.safeAreaInsets().top)
}
