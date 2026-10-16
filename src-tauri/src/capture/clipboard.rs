//! Save clipboard (Step 6.3), also the second half of Send selection. Reads the general pasteboard
//! with the drop's precedence; password managers' clipboards are never saved.
use tauri::AppHandle;

use super::{capture_blocking, failed, input_from, skipped, CaptureSource};
use crate::db::settings;
use crate::model::CaptureResult;
use crate::platform::macos::pasteboard::{Board, PbContent};
use crate::platform::macos::{frontmost, on_main, pasteboard};

/// `source_kind`: `clipboard` or `selection`.
pub async fn run(app: &AppHandle, source_kind: &'static str) -> CaptureResult {
    let read = on_main(app, |_| {
        let read = pasteboard::read(Board::General);
        let image = match read.content {
            Some(PbContent::Url { .. }) => pasteboard::read_image(Board::General),
            _ => None,
        };
        (read, image, frontmost::frontmost_app())
    })
    .await;
    let (read, image, front) = match read {
        Ok(v) => v,
        Err(e) => return failed(app, e.to_string()),
    };
    let excluded: Vec<String> = settings::read(app, "capture.excludedApps").unwrap_or_default();
    let front_excluded = front
        .as_ref()
        .and_then(|f| f.bundle_id.as_deref())
        .is_some_and(|id| excluded.iter().any(|e| e == id));
    if read.concealed() || front_excluded {
        let name = front.as_ref().and_then(|f| f.name.clone().or_else(|| f.bundle_id.clone()));
        return skipped(Some("notice.clipboardExcluded"), &[("app", name.unwrap_or_default())]);
    }
    let Some(content) = read.content else {
        return skipped(Some("notice.clipboardEmpty"), &[]);
    };
    capture_blocking(app, input_from(content, image), CaptureSource::new(source_kind, front)).await
}
