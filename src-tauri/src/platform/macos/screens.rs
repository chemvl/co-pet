//! Displays in one coordinate space: global logical points with a top-left origin at the primary
//! (menu bar) display, y growing downwards. AppKit uses a bottom-left origin; the conversions live
//! here and nowhere else.
use objc2::MainThreadMarker;
use objc2_app_kit::{NSEvent, NSScreen, NSWindow};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use crate::model::Rect;

#[derive(Debug, Clone, PartialEq)]
pub struct Screen {
    /// `name:WxH@x,y`; unique within one monitor setup.
    pub id: String,
    /// `NSScreen.localizedName`.
    pub name: String,
    pub frame: Rect,
    /// `NSScreen.visibleFrame`: the frame minus the menu bar and the Dock.
    pub visible: Rect,
    pub scale: f64,
}

/// Height of the primary display, which anchors the AppKit ↔ top-left flip.
fn primary_height(mtm: MainThreadMarker) -> f64 {
    NSScreen::screens(mtm).iter().next().map_or(0.0, |s| s.frame().size.height)
}

fn to_top_left(r: NSRect, primary_h: f64) -> Rect {
    Rect {
        x: r.origin.x,
        y: primary_h - (r.origin.y + r.size.height),
        w: r.size.width,
        h: r.size.height,
    }
}

fn to_cocoa(r: Rect, primary_h: f64) -> NSRect {
    NSRect::new(NSPoint::new(r.x, primary_h - (r.y + r.h)), NSSize::new(r.w, r.h))
}

/// Every display, primary first (AppKit order).
pub fn screens(mtm: MainThreadMarker) -> Vec<Screen> {
    let all = NSScreen::screens(mtm);
    let primary_h = all.iter().next().map_or(0.0, |s| s.frame().size.height);
    all.iter()
        .map(|s| {
            let frame = to_top_left(s.frame(), primary_h);
            let name = s.localizedName().to_string();
            Screen {
                id: format!(
                    "{name}:{}x{}@{},{}",
                    frame.w.round() as i64,
                    frame.h.round() as i64,
                    frame.x.round() as i64,
                    frame.y.round() as i64
                ),
                name,
                frame,
                visible: to_top_left(s.visibleFrame(), primary_h),
                scale: s.backingScaleFactor(),
            }
        })
        .collect()
}

/// Sorted screen ids joined by `|`: the key under which `pet.positions` remembers a setup.
pub fn setup_key(screens: &[Screen]) -> String {
    let mut ids: Vec<&str> = screens.iter().map(|s| s.id.as_str()).collect();
    ids.sort_unstable();
    ids.join("|")
}

/// The cursor in global top-left points.
pub fn mouse_location(mtm: MainThreadMarker) -> (f64, f64) {
    let p = NSEvent::mouseLocation();
    (p.x, primary_height(mtm) - p.y)
}

/// An AppKit screen point (bottom-left origin) in global top-left points.
pub fn point_to_top_left(mtm: MainThreadMarker, p: NSPoint) -> (f64, f64) {
    (p.x, primary_height(mtm) - p.y)
}

pub fn contains(r: &Rect, x: f64, y: f64) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

/// The window's frame in global top-left points.
pub fn window_frame(mtm: MainThreadMarker, window: &NSWindow) -> Rect {
    to_top_left(window.frame(), primary_height(mtm))
}

/// Moves and resizes `window` to `frame` (global top-left points).
pub fn set_window_frame(mtm: MainThreadMarker, window: &NSWindow, frame: Rect) {
    window.setFrame_display(to_cocoa(frame, primary_height(mtm)), true);
}
