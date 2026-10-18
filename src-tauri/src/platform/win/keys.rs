//! Synthetic Ctrl+<key> presses (Send selection's Ctrl+C, Paste into field's Ctrl+V). `SendInput`
//! can't reach apps running elevated (UIPI).
use std::time::{Duration, Instant};

use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use tauri::AppHandle;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT};

use crate::error::{AppError, AppResult};

const RELEASE_WAIT: Duration = Duration::from_secs(1);

fn any_modifier_down() -> bool {
    [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN]
        .iter()
        .any(|vk| unsafe { GetAsyncKeyState(i32::from(vk.0)) } as u16 & 0x8000 != 0)
}

/// Waits (≤ 1 s) until Shift/Ctrl/Alt/Win are up, so a hotkey's modifiers don't mix into Ctrl+C.
pub async fn wait_modifiers_released() {
    let start = Instant::now();
    while any_modifier_down() && start.elapsed() < RELEASE_WAIT {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Ctrl+`ch` to the foreground app (⌘ on macOS).
pub async fn command_key(app: &AppHandle, ch: char) -> AppResult<()> {
    crate::platform::on_main(app, move |_| {
        let mut enigo = Enigo::new(&Settings::default()).map_err(|e| AppError::msg(format!("keys: {e}")))?;
        enigo.key(Key::Control, Direction::Press).map_err(|e| AppError::msg(format!("keys: {e}")))?;
        let r = enigo.key(Key::Unicode(ch), Direction::Click);
        enigo.key(Key::Control, Direction::Release).map_err(|e| AppError::msg(format!("keys: {e}")))?;
        r.map_err(|e| AppError::msg(format!("keys: {e}")))
    })
    .await?
}
