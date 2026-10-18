//! Displays in one coordinate space: global units with a top-left origin at the primary display,
//! y growing downwards. Units are logical points on macOS and physical pixels on Windows; the OS
//! conversions live in the backend's `screens` and nowhere else.
use crate::model::Rect;

pub use super::os::screens::{mouse_location, screens};

#[derive(Debug, Clone, PartialEq)]
pub struct Screen {
    /// `name:WxH@x,y`; unique within one monitor setup.
    pub id: String,
    /// The display's localized name.
    pub name: String,
    pub frame: Rect,
    /// The work area: the frame minus the menu bar and the Dock (taskbar on Windows).
    pub visible: Rect,
    pub scale: f64,
}

/// Sorted screen ids joined by `|`: the key under which `pet.positions` remembers a setup.
pub fn setup_key(screens: &[Screen]) -> String {
    let mut ids: Vec<&str> = screens.iter().map(|s| s.id.as_str()).collect();
    ids.sort_unstable();
    ids.join("|")
}

pub fn contains(r: &Rect, x: f64, y: f64) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

/// Global units per CSS pixel on `s`: 1.0 on macOS (points), `s.scale` on Windows (pixels).
pub fn css_scale(s: &Screen) -> f64 {
    if cfg!(windows) {
        s.scale
    } else {
        1.0
    }
}
