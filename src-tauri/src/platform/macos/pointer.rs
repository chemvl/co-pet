//! macOS half of the pointer loop (`platform::pointer`): the mouse button, OS drag detection by
//! the drag pasteboard, and the drop catcher panel.
use objc2::MainThreadMarker;
use objc2_app_kit::NSEvent;
use tauri::{Manager, WebviewWindow};
use tauri_nspanel::ManagerExt;

use super::{drop_target, pasteboard};
use crate::model::Rect;
use crate::platform::pasteboard::Board;

pub(in crate::platform) fn left_button_down() -> bool {
    NSEvent::pressedMouseButtons() & 1 != 0
}

/// Detects an OS drag in progress: left button down while the drag pasteboard changed since the
/// last button-up.
#[derive(Default)]
pub(in crate::platform) struct Probe {
    button_down: bool,
    /// Drag pasteboard change count at the last left-button-up.
    drag_baseline: Option<isize>,
}

impl Probe {
    pub(in crate::platform) fn update(&mut self, left_down: bool, _cursor: (f64, f64), _over_hit_region: bool) -> bool {
        if left_down {
            self.button_down = true;
            let baseline = *self.drag_baseline.get_or_insert_with(|| pasteboard::change_count(Board::Drag));
            pasteboard::change_count(Board::Drag) != baseline
        } else {
            if self.button_down || self.drag_baseline.is_none() {
                self.drag_baseline = Some(pasteboard::change_count(Board::Drag));
            }
            self.button_down = false;
            false
        }
    }
}

/// After each tick: keep the catcher panel over `catcher` (global top-left points) just below the
/// pet panel, or hide it, and track whether an OS drag is in progress.
pub(in crate::platform) fn after_tick(
    mtm: MainThreadMarker,
    w: &WebviewWindow,
    catcher: Option<Rect>,
    dragging: bool,
    may_restore: bool,
) {
    if let Ok(panel) = w.app_handle().get_webview_panel(w.label()) {
        drop_target::place_catcher(mtm, panel.as_panel(), catcher);
    }
    drop_target::set_dragging(mtm, dragging, may_restore);
}
