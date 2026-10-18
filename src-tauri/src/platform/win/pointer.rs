//! Windows half of the pointer loop (`platform::pointer`): the mouse button, OS drag detection by
//! `DragProbe` (D4), and the drop catcher window.
use tauri::WebviewWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};

use crate::model::Rect;
use crate::platform::drag_probe::DragProbe;
use crate::platform::MainThread;

pub(in crate::platform) fn left_button_down() -> bool {
    let state = unsafe { GetAsyncKeyState(i32::from(VK_LBUTTON.0)) };
    state as u16 & 0x8000 != 0
}

#[derive(Default)]
pub(in crate::platform) struct Probe(DragProbe);

impl Probe {
    pub(in crate::platform) fn update(&mut self, left_down: bool, cursor: (f64, f64), over_hit_region: bool) -> bool {
        self.0.update(left_down, cursor, over_hit_region)
    }
}

/// Shows the drop catcher over pet + halo while an OS drag is in progress, hides it otherwise.
pub(in crate::platform) fn after_tick(
    mt: MainThread,
    _w: &WebviewWindow,
    catcher: Option<Rect>,
    dragging: bool,
    _may_restore: bool,
) {
    super::drop_target::place_catcher(mt, catcher.filter(|_| dragging));
}
