//! NSPasteboard half of `platform::pasteboard`.
use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::PathBuf;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ProtocolObject};
use objc2::ClassType;
use objc2_app_kit::{
    NSPasteboard, NSPasteboardItem, NSPasteboardNameDrag, NSPasteboardReadingOptionKey,
    NSPasteboardTypePNG, NSPasteboardURLReadingFileURLsOnlyKey, NSPasteboardWriting,
};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSNumber, NSString, NSURL};
use tauri::AppHandle;

use crate::error::{AppError, AppResult};
use crate::platform::pasteboard::{
    text_content, Board, PbContent, PbRead, TYPE_CONCEALED, TYPE_HTML, TYPE_PLAIN, TYPE_TIFF, TYPE_TRANSIENT,
    TYPE_PNG, TYPE_URL, TYPE_URL_NAME,
};

fn board(b: Board) -> Retained<NSPasteboard> {
    match b {
        Board::General => NSPasteboard::generalPasteboard(),
        // SAFETY: AppKit's exported pasteboard name constant.
        Board::Drag => NSPasteboard::pasteboardWithName(unsafe { NSPasteboardNameDrag }),
    }
}

pub fn change_count(b: Board) -> isize {
    board(b).changeCount()
}

/// Reads the board with Step 6.2 precedence: file URLs → `public.url` → PNG/TIFF → HTML + plain
/// text. Plain text that is a single http(s) URL becomes `Url`.
pub fn read(b: Board) -> PbRead {
    read_from(&board(b))
}

/// The board's image as PNG (TIFF converted), whatever `read` picked. Browser image drags carry
/// `public.url` as well as the image, and `read` returns the `Url`; capture prefers the image.
pub fn read_image(b: Board) -> Option<Vec<u8>> {
    image_from(&board(b))
}

/// Replaces the general pasteboard: file URLs (one item each) or a single item carrying every
/// `data` UTI → string pair, plus optional PNG bytes. With files, `data` and `png` join the first
/// item.
pub fn write(files: &[PathBuf], data: &BTreeMap<String, String>, png: Option<&[u8]>) -> AppResult<()> {
    write_to(&NSPasteboard::generalPasteboard(), files, data, png)
}

pub async fn write_on_main(
    app: &AppHandle,
    files: Vec<PathBuf>,
    data: BTreeMap<String, String>,
    png: Option<Vec<u8>>,
) -> AppResult<()> {
    crate::platform::on_main(app, move |_| write(&files, &data, png.as_deref())).await?
}

/// `text` as plain text marked concealed and transient (nspasteboard.org), so Save clipboard and
/// clipboard managers don't keep it.
fn concealed_text(text: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (TYPE_PLAIN.to_string(), text.to_string()),
        (TYPE_CONCEALED.to_string(), String::new()),
        (TYPE_TRANSIENT.to_string(), String::new()),
    ])
}

/// Copies a secret (the recovery key) to the general pasteboard; see [`concealed_text`].
pub async fn write_concealed_text(app: &AppHandle, text: String) -> AppResult<()> {
    write_on_main(app, Vec::new(), concealed_text(&text), None).await
}

fn read_from(pb: &NSPasteboard) -> PbRead {
    let change_count = pb.changeCount();
    let types: Vec<String> =
        pb.types().map(|t| t.iter().map(|s| s.to_string()).collect()).unwrap_or_default();
    let has = |t: &str| types.iter().any(|x| x == t);

    let content = 'found: {
        let files = file_urls(pb);
        if !files.is_empty() {
            break 'found Some(PbContent::Files(files));
        }
        if let Some(url) = string(pb, TYPE_URL).map(|u| u.trim().to_string()).filter(|u| !u.is_empty()) {
            let title = string(pb, TYPE_URL_NAME).map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
            break 'found Some(PbContent::Url { url, title });
        }
        if let Some(png) = image_from(pb) {
            break 'found Some(PbContent::Image(png));
        }
        let plain = string(pb, TYPE_PLAIN).filter(|p| !p.trim().is_empty());
        let html = if has(TYPE_HTML) { string(pb, TYPE_HTML).filter(|h| !h.trim().is_empty()) } else { None };
        plain.map(|plain| text_content(plain, html))
    };
    PbRead { content, types, change_count }
}

fn image_from(pb: &NSPasteboard) -> Option<Vec<u8>> {
    if let Some(png) = data(pb, TYPE_PNG) {
        return Some(png);
    }
    match data(pb, TYPE_TIFF).map(|t| tiff_to_png(&t)) {
        Some(Ok(png)) => Some(png),
        Some(Err(e)) => {
            eprintln!("[pasteboard] tiff → png failed: {e}");
            None
        }
        None => None,
    }
}

fn file_urls(pb: &NSPasteboard) -> Vec<PathBuf> {
    let classes: Retained<NSArray<AnyClass>> = NSArray::from_slice(&[NSURL::class()]);
    let yes = NSNumber::new_bool(true);
    // SAFETY: AppKit's exported option key constant.
    let key: &NSPasteboardReadingOptionKey = unsafe { NSPasteboardURLReadingFileURLsOnlyKey };
    let options: Retained<NSDictionary<NSPasteboardReadingOptionKey, AnyObject>> =
        NSDictionary::from_slices(&[key], &[yes.as_ref()]);
    // SAFETY: the class list holds NSURL (an NSPasteboardReading class) and the options dictionary
    // uses a documented key with an NSNumber value.
    let Some(objects) = (unsafe { pb.readObjectsForClasses_options(&classes, Some(&options)) }) else {
        return Vec::new();
    };
    objects
        .iter()
        .filter_map(|o| o.downcast::<NSURL>().ok())
        // Finder drags carry file reference URLs (`file:///.file/id=…`); resolve them to paths.
        .filter_map(|u| u.filePathURL().and_then(|p| p.path()))
        .map(|p| PathBuf::from(p.to_string()))
        .collect()
}

fn string(pb: &NSPasteboard, uti: &str) -> Option<String> {
    pb.stringForType(&NSString::from_str(uti)).map(|s| s.to_string())
}

fn data(pb: &NSPasteboard, uti: &str) -> Option<Vec<u8>> {
    pb.dataForType(&NSString::from_str(uti)).map(|d| d.to_vec()).filter(|d| !d.is_empty())
}

fn tiff_to_png(tiff: &[u8]) -> Result<Vec<u8>, image::ImageError> {
    let img = image::load_from_memory_with_format(tiff, image::ImageFormat::Tiff)?;
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)?;
    Ok(out.into_inner())
}

fn write_to(
    pb: &NSPasteboard,
    files: &[PathBuf],
    data: &BTreeMap<String, String>,
    png: Option<&[u8]>,
) -> AppResult<()> {
    let objects: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = if files.is_empty() {
        let item = NSPasteboardItem::new();
        for (uti, value) in data {
            item.setString_forType(&NSString::from_str(value), &NSString::from_str(uti));
        }
        if let Some(png) = png {
            // SAFETY: AppKit's exported type constant.
            item.setData_forType(&NSData::with_bytes(png), unsafe { NSPasteboardTypePNG });
        }
        vec![ProtocolObject::from_retained(item)]
    } else {
        files
            .iter()
            .map(|f| {
                let path = f.to_str().ok_or_else(|| AppError::msg(format!("non-UTF-8 path {f:?}")))?;
                Ok(ProtocolObject::from_retained(NSURL::fileURLWithPath(&NSString::from_str(path))))
            })
            .collect::<AppResult<_>>()?
    };
    if objects.is_empty() {
        return Err(AppError::msg("nothing to write to the pasteboard"));
    }

    pb.clearContents();
    if !pb.writeObjects(&NSArray::from_retained_slice(&objects)) {
        return Err(AppError::msg("pasteboard rejected the items"));
    }
    if !files.is_empty() {
        // `setString:forType:` / `setData:forType:` add to the first pasteboard item.
        for (uti, value) in data {
            pb.setString_forType(&NSString::from_str(value), &NSString::from_str(uti));
        }
        if let Some(png) = png {
            // SAFETY: AppKit's exported type constant.
            pb.setData_forType(Some(&NSData::with_bytes(png)), unsafe { NSPasteboardTypePNG });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concealed_text_is_skipped_by_clipboard_capture() {
        // A private pasteboard, so the test leaves the user's clipboard alone.
        let pb = NSPasteboard::pasteboardWithUniqueName();
        write_to(&pb, &[], &concealed_text("ABCD-EFGH"), None).unwrap();
        let read = read_from(&pb);
        // `capture::clipboard::run` skips a board that reads as concealed.
        assert!(read.concealed());
        assert!(read.types.iter().any(|t| t == TYPE_TRANSIENT));
        assert!(matches!(read.content, Some(PbContent::Text { ref plain, .. }) if plain == "ABCD-EFGH"));
    }
}
