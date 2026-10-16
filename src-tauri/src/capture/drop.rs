//! Drop on the pet (Step 6.2). The native drag pasteboard is read first: it carries file URLs and
//! typed data the DOM `DataTransfer` hides. The DOM payload is the fallback.
use tauri::AppHandle;

use super::{capture_blocking, failed, input_from, is_http_url, skipped, CaptureInput, CaptureSource};
use crate::model::{CaptureResult, DropPayload};
use crate::platform::macos::{frontmost, on_main, pasteboard};
use crate::platform::macos::pasteboard::{Board, PbContent};

pub async fn run(app: &AppHandle, p: DropPayload) -> CaptureResult {
    // The pet panel never activates Co-pet, so the frontmost app is the drag's source.
    let read = on_main(app, |_| {
        let read = pasteboard::read(Board::Drag);
        let image = match read.content {
            Some(PbContent::Url { .. }) => pasteboard::read_image(Board::Drag),
            _ => None,
        };
        (read.content, image, frontmost::frontmost_app())
    })
    .await;
    let (content, image, front) = match read {
        Ok(v) => v,
        Err(e) => return failed(app, e.to_string()),
    };
    let input = match content {
        Some(c) => input_from(c, image),
        None => match dom_input(&p) {
            Some(input) => input,
            // Files the drag pasteboard didn't expose (file promises); the DOM only has names.
            None if !p.file_names.is_empty() => {
                return failed(app, format!("couldn't read {}", p.file_names.join(", ")));
            }
            None => return skipped(None, &[]),
        },
    };
    capture_blocking(app, input, CaptureSource::new("drop", front)).await
}

/// The DOM fallback: an http(s) entry of `text/uri-list` is a link; otherwise the plain text
/// (with its HTML), where a single http(s) URL is a link too.
fn dom_input(p: &DropPayload) -> Option<CaptureInput> {
    let link = p
        .uri_list
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('#') && is_http_url(l));
    if let Some(url) = link {
        return Some(CaptureInput::Url { url: url.to_string(), title: None });
    }
    let plain = p.plain.trim();
    if plain.is_empty() {
        return None;
    }
    if is_http_url(plain) {
        return Some(CaptureInput::Url { url: plain.to_string(), title: None });
    }
    let html = Some(p.html.clone()).filter(|h| !h.trim().is_empty());
    Some(CaptureInput::Text { plain: p.plain.clone(), html })
}
