//! D11: Snipping Tool's region overlay (`ms-screenclip:`) puts the capture on the clipboard;
//! Co-pet waits for the clipboard to change to an image. Esc leaves the clipboard unchanged.
use std::path::Path;
use std::time::{Duration, Instant};

use tauri::AppHandle;

use super::pasteboard;
use crate::error::{AppError, AppResult};
use crate::platform::pasteboard::Board;

const WAIT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(150);
/// Re-reads after a changed clipboard yields no image (Snipping Tool may still hold it open):
/// about 750 ms at [`POLL`].
const READ_RETRIES: u32 = 5;

/// Lets the user capture a region into the PNG `out`. `Ok(false)` when cancelled: no image
/// within [`WAIT`], or the clipboard changed to something that isn't an image.
pub async fn interactive(app: &AppHandle, out: &Path) -> AppResult<bool> {
    use tauri_plugin_opener::OpenerExt;
    let before = pasteboard::change_count(Board::General);
    app.opener()
        .open_url("ms-screenclip:", None::<&str>)
        .map_err(|e| AppError::msg(format!("screenclip: {e}")))?;
    let start = Instant::now();
    let mut misses = 0;
    while start.elapsed() < WAIT {
        tokio::time::sleep(POLL).await;
        // Once the clipboard has changed, keep re-reading without waiting for another change.
        if misses == 0 && pasteboard::change_count(Board::General) == before {
            continue;
        }
        let png = tauri::async_runtime::spawn_blocking(|| pasteboard::read_image(Board::General))
            .await
            .ok()
            .flatten();
        match png {
            Some(png) => {
                tokio::fs::write(out, png).await?;
                return Ok(true);
            }
            None if misses < READ_RETRIES => misses += 1,
            None => return Ok(false), // the clipboard changed to something else: the user moved on
        }
    }
    Ok(false)
}
