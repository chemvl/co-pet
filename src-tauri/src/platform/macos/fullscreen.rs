//! Full-screen detection for `platform::fullscreen`. On a display with a camera notch a full-screen
//! window starts below the notch (`safeAreaInsets.top`), so it covers the frame minus that strip
//! rather than the whole frame.
use objc2::MainThreadMarker;
use objc2_app_kit::NSScreen;

use super::{frontmost, window_titles};
use crate::model::Rect;

/// `kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements`.
const LIST_OPTIONS: u32 = (1 << 0) | (1 << 4);
/// Window bounds and screen frames are both in points; allow rounding.
const TOLERANCE: f64 = 1.0;

/// Whether some on-screen, layer-0 window of the frontmost app covers `frame` (global top-left
/// points, the space `CGWindowListCopyWindowInfo` reports bounds in), or all of it below the notch.
pub(in crate::platform) fn frontmost_fullscreen_on(mtm: MainThreadMarker, frame: Rect) -> bool {
    let Some(front_pid) = frontmost::frontmost_pid() else { return false };
    if front_pid == std::process::id() as i32 {
        return false;
    }
    let near = |a: f64, b: f64| (a - b).abs() <= TOLERANCE;
    let notch = notch_height(mtm, frame);
    window_titles::window_list(LIST_OPTIONS, &[front_pid], false).iter().any(|w| {
        if w.layer != 0 {
            return false;
        }
        let Some(r) = w.bounds else { return false };
        let top = r.y - frame.y;
        near(r.x, frame.x)
            && near(r.w, frame.w)
            && near(r.y + r.h, frame.y + frame.h)
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
