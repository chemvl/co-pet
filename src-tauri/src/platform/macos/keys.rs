//! Synthetic Cmd+<key> presses (Send selection's Cmd+C, Paste into field's Cmd+V). Needs the
//! Accessibility permission; callers check `permissions::accessibility_trusted()` first.
use std::time::Duration;

use enigo::Direction::{Click, Press, Release};
use enigo::{Enigo, Key, Keyboard, Settings};
use tauri::AppHandle;

use crate::error::{AppError, AppResult};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceFlagsState(state_id: i32) -> u64;
}

/// `kCGEventSourceStateHIDSystemState`: the physical keyboard, not synthetic events.
const HID_SYSTEM_STATE: i32 = 1;
/// `kCGEventFlagMaskShift | Control | Alternate | Command`.
const MODIFIER_MASK: u64 = 0x0002_0000 | 0x0004_0000 | 0x0008_0000 | 0x0010_0000;
const RELEASE_POLL: Duration = Duration::from_millis(20);
const RELEASE_WAIT: Duration = Duration::from_secs(1);

fn modifiers_down() -> bool {
    // SAFETY: plain query of the HID system event source state.
    unsafe { CGEventSourceFlagsState(HID_SYSTEM_STATE) & MODIFIER_MASK != 0 }
}

/// Waits (up to 1 s) until the user lets go of Shift/Control/Option/Command, so a synthetic Cmd+C
/// sent right after a hotkey is not merged with the hotkey's still-held modifiers.
pub async fn wait_modifiers_released() {
    let deadline = tokio::time::Instant::now() + RELEASE_WAIT;
    while modifiers_down() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(RELEASE_POLL).await;
    }
}

/// Posts Cmd+`ch` to the frontmost app. Runs on the main thread: enigo resolves `ch` through the
/// current keyboard layout (Text Input Sources), which is main-thread-only.
pub async fn command_key(app: &AppHandle, ch: char) -> AppResult<()> {
    super::on_main(app, move |_| post_command_key(ch)).await?
}

fn post_command_key(ch: char) -> AppResult<()> {
    let err = |e: &dyn std::fmt::Display| AppError::msg(format!("couldn't send ⌘{ch}: {e}"));
    let settings = Settings { open_prompt_to_get_permissions: false, ..Settings::default() };
    let mut enigo = Enigo::new(&settings).map_err(|e| err(&e))?;
    enigo.key(Key::Meta, Press).map_err(|e| err(&e))?;
    let clicked = enigo.key(Key::Unicode(ch), Click);
    let released = enigo.key(Key::Meta, Release);
    clicked.and(released).map_err(|e| err(&e))
}
