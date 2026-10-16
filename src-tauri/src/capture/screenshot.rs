//! Capture screen (Step 6.5): the system's interactive `screencapture`, then a `screenshot` item.
use tauri::{AppHandle, Manager};

use super::{capture_blocking, failed, skipped, CaptureInput, CaptureSource};
use crate::db::new_id;
use crate::model::CaptureResult;
use crate::paths::Paths;
use crate::platform::macos::permissions;

pub async fn run(app: &AppHandle) -> CaptureResult {
    if !permissions::screen_recording_granted() {
        return skipped(Some("notice.needsScreenRecording"), &[]);
    }
    let tmp = app.state::<Paths>().tmp.clone();
    if let Err(e) = tokio::fs::create_dir_all(&tmp).await {
        return failed(app, e.to_string());
    }
    let shot = tmp.join(format!("shot-{}.png", new_id()));
    let status = tokio::process::Command::new("/usr/sbin/screencapture")
        .args(["-i", "-x", "-t", "png"])
        .arg(&shot)
        .status()
        .await;
    if let Err(e) = status {
        return failed(app, format!("screencapture: {e}"));
    }
    // Esc leaves no file (or an empty one): nothing to save, nothing to say.
    let taken = tokio::fs::metadata(&shot).await.is_ok_and(|m| m.len() > 0);
    if !taken {
        let _ = tokio::fs::remove_file(&shot).await;
        return skipped(None, &[]);
    }
    capture_blocking(app, CaptureInput::Screenshot { path: shot }, CaptureSource::new("screenshot", None)).await
}
