//! Send selection (Step 6.6, hotkey): Cmd+C in the frontmost app, then the clipboard capture with
//! source `selection`. The previous clipboard is not restored.
use std::time::Duration;

use tauri::AppHandle;

use super::{clipboard, failed, skipped};
use crate::model::CaptureResult;
use crate::platform::macos::pasteboard::{self, Board};
use crate::platform::macos::{keys, on_main, permissions};

const POLL: Duration = Duration::from_millis(20);
const COPY_TIMEOUT: Duration = Duration::from_millis(500);

pub async fn run(app: &AppHandle) -> CaptureResult {
    if !permissions::accessibility_trusted() {
        return skipped(Some("notice.needsAccessibility"), &[]);
    }
    // The hotkey's Shift/Cmd may still be held; Cmd+C must arrive alone.
    keys::wait_modifiers_released().await;
    let before = match on_main(app, |_| pasteboard::change_count(Board::General)).await {
        Ok(n) => n,
        Err(e) => return failed(app, e.to_string()),
    };
    if let Err(e) = keys::command_key(app, 'c').await {
        return failed(app, e.to_string());
    }
    let deadline = tokio::time::Instant::now() + COPY_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(POLL).await;
        match on_main(app, |_| pasteboard::change_count(Board::General)).await {
            Ok(n) if n != before => return clipboard::run(app, "selection").await,
            Ok(_) => {}
            Err(e) => return failed(app, e.to_string()),
        }
    }
    skipped(Some("notice.noSelection"), &[])
}
