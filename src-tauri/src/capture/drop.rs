//! Drop on the pet (Step 6.2). The native drop target (`platform::drop_target`) accepts the
//! drag synchronously, reads the drag data with [`read`] inside the drop callback
//! (`performDragOperation` on macOS, `IDropTarget::Drop` on Windows), and hands the result to
//! [`spawn`], which captures off the main thread and emits `capture-result` to the pet window.
//! The webview never sees the drop.
use tauri::{AppHandle, Manager};

use super::{capture_blocking, input_from, CaptureSource};
use crate::db::{items, Db};
use crate::events;
use crate::model::CaptureResult;
use crate::platform::frontmost::FrontApp;
use crate::platform::pasteboard::{self, Board, PbContent};
use crate::transcribe::worker::is_media;

/// What was dropped, read while the drag session is still alive.
pub struct Dropped {
    content: PbContent,
    /// The board's image when it also carries a URL: a browser image drag saves the picture.
    image: Option<Vec<u8>>,
    source: Option<FrontApp>,
}

/// Reads the drag board (the macOS drag pasteboard, or the dropped `IDataObject` on Windows).
/// Main thread only. `source` is the app the drag came from, recorded when the drag started (the
/// pet window never activates Co-pet). `None` when nothing usable is on the board (e.g. file
/// promises), so the drop is refused and the drag snaps back.
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
        let mut result = capture_blocking(&app, input, CaptureSource::new("drop", source)).await;
        offer_transcribe(&app, &mut result);
        events::capture_result(&app, result);
    });
}

/// One saved audio or video file: the pet offers to transcribe it.
fn offer_transcribe(app: &AppHandle, result: &mut CaptureResult) {
    let [id] = result.item_ids.as_slice() else { return };
    if result.status != "saved" {
        return;
    }
    let row = match items::get_row(&app.state::<Db>().conn(), id) {
        Ok(row) => row,
        Err(e) => {
            eprintln!("[drop] reading {id}: {e}");
            return;
        }
    };
    if !row.mime.as_deref().is_some_and(is_media) {
        return;
    }
    result.message = Some("notice.offerTranscribe".into());
    result.message_params.insert("title".into(), row.file_name.unwrap_or(row.title));
    result.transcribe_item_id = Some(id.clone());
}
