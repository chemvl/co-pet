//! Drop on the pet (Step 6.2). The native drop target (`platform::macos::drop_target`) accepts the
//! drag synchronously, reads the drag pasteboard with [`read`] inside `performDragOperation`, and
//! hands the result to [`spawn`], which captures off the main thread and emits `capture-result`
//! to the pet window. WebKit never sees the drop.
use tauri::AppHandle;

use super::{capture_blocking, input_from, CaptureSource};
use crate::events;
use crate::platform::macos::frontmost::FrontApp;
use crate::platform::macos::pasteboard::{self, Board, PbContent};

/// What was dropped, read while the drag session is still alive.
pub struct Dropped {
    content: PbContent,
    /// The board's image when it also carries a URL: a browser image drag saves the picture.
    image: Option<Vec<u8>>,
    source: Option<FrontApp>,
}

/// Reads the drag pasteboard. Main thread only (AppKit). `source` is the app the drag came from,
/// recorded when the drag started (the pet panel never activates Co-pet). `None` when nothing
/// usable is on the board (e.g. file promises), so the drop is refused and the drag snaps back.
pub fn read(source: Option<FrontApp>) -> Option<Dropped> {
    let content = pasteboard::read(Board::Drag).content?;
    let image = match content {
        PbContent::Url { .. } => pasteboard::read_image(Board::Drag),
        _ => None,
    };
    Some(Dropped { content, image, source })
}

/// Captures the drop on the blocking pool and emits `capture-result` to the pet window.
pub fn spawn(app: &AppHandle, dropped: Dropped) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Dropped { content, image, source } = dropped;
        let input = input_from(content, image);
        let result = capture_blocking(&app, input, CaptureSource::new("drop", source)).await;
        events::capture_result(&app, result);
    });
}
