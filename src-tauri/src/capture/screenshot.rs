//! Capture screen (Step 6.5): the system's interactive `screencapture`, then a `screenshot` item.
use tauri::{AppHandle, Manager};

use super::{capture_blocking, failed, skipped, CaptureInput, CaptureSource};
use crate::db::new_id;
use crate::model::CaptureResult;
use crate::paths::Paths;
use crate::platform::{permissions, screenshot};

pub async fn run(app: &AppHandle) -> CaptureResult {
    if !permissions::screen_recording_granted() {
        return skipped(Some("notice.needsScreenRecording"), &[]);
    }
    let tmp = app.state::<Paths>().tmp.clone();
    if let Err(e) = tokio::fs::create_dir_all(&tmp).await {
        return failed(app, e.to_string());
    }
    let shot = tmp.join(format!("shot-{}.png", new_id()));
    match screenshot::interactive(app, &shot).await {
        Err(e) => return failed(app, e.to_string()),
        // Esc leaves no file (or an empty one): nothing to save, nothing to say.
        Ok(false) => return skipped(None, &[]),
        Ok(true) => {}
    }
    capture_blocking(app, CaptureInput::Screenshot { path: shot }, CaptureSource::new("screenshot", None)).await
}
