//! The system's interactive screenshot: `screencapture -i`.
use std::path::Path;

use tauri::AppHandle;

use crate::error::{AppError, AppResult};

/// Lets the user capture a region or window into the PNG `out`. `Ok(false)` when cancelled
/// (Esc leaves no file, or an empty one, which is removed).
pub async fn interactive(_app: &AppHandle, out: &Path) -> AppResult<bool> {
    let status = tokio::process::Command::new("/usr/sbin/screencapture")
        .args(["-i", "-x", "-t", "png"])
        .arg(out)
        .status()
        .await;
    if let Err(e) = status {
        return Err(AppError::msg(format!("screencapture: {e}")));
    }
    let taken = tokio::fs::metadata(out).await.is_ok_and(|m| m.len() > 0);
    if !taken {
        let _ = tokio::fs::remove_file(out).await;
    }
    Ok(taken)
}
