//! Windows clipboard, and the drag data of a drop in progress (`Board::Drag`), through one reader.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;

use tauri::AppHandle;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardFormatNameW,
    GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GHND};
use windows::Win32::System::Ole::{CF_DIB, CF_DIBV5, CF_HDROP, CF_UNICODETEXT};

use crate::error::{AppError, AppResult};
use crate::platform::clip;
use crate::platform::pasteboard::{
    text_content, Board, PbContent, PbRead, TYPE_CONCEALED, TYPE_HTML, TYPE_PLAIN, TYPE_TRANSIENT, TYPE_URL,
    TYPE_URL_NAME,
};

/// A source of clipboard-format bytes: the clipboard, or a drop's `IDataObject`.
pub trait Formats {
    fn list(&self) -> Vec<u32>;
    fn bytes(&self, fmt: u32) -> Option<Vec<u8>>;
}

thread_local! {
    /// The `IDataObject` of the drop being performed (main thread, inside `IDropTarget::Drop`).
    static DRAG: RefCell<Option<Box<dyn Formats>>> = const { RefCell::new(None) };
}
/// Incremented on every drag enter; `change_count(Board::Drag)`.
static DRAG_COUNT: AtomicIsize = AtomicIsize::new(0);

/// Sets (on drag enter) or clears (on leave/drop) the source `read(Board::Drag)` reads.
#[allow(dead_code)] // called by `win::drop_target` (Task 11)
pub fn set_drag_source(src: Option<Box<dyn Formats>>) {
    if src.is_some() {
        DRAG_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    DRAG.with(|d| *d.borrow_mut() = src);
}

/// The id of a registered clipboard format (registering it on first use).
pub(crate) fn registered(name: &str) -> u32 {
    let w = crate::platform::wide(name);
    unsafe { RegisterClipboardFormatW(PCWSTR(w.as_ptr())) }
}

fn format_name(fmt: u32) -> String {
    match fmt {
        f if f == CF_HDROP.0 as u32 => "CF_HDROP".into(),
        f if f == CF_UNICODETEXT.0 as u32 => "CF_UNICODETEXT".into(),
        f if f == CF_DIB.0 as u32 => "CF_DIB".into(),
        f if f == CF_DIBV5.0 as u32 => "CF_DIBV5".into(),
        f => {
            let mut buf = [0u16; 128];
            let n = unsafe { GetClipboardFormatNameW(f, &mut buf) };
            if n > 0 {
                String::from_utf16_lossy(&buf[..n as usize])
            } else {
                format!("#{f}")
            }
        }
    }
}

/// The open clipboard; closed on drop.
struct Clipboard;

impl Clipboard {
    /// Another app may hold the clipboard briefly: retry for up to 200 ms.
    fn open() -> AppResult<Self> {
        for _ in 0..20 {
            if unsafe { OpenClipboard(None) }.is_ok() {
                return Ok(Clipboard);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Err(AppError::msg("clipboard busy"))
    }
}

impl Drop for Clipboard {
    fn drop(&mut self) {
        let _ = unsafe { CloseClipboard() };
    }
}

impl Formats for Clipboard {
    fn list(&self) -> Vec<u32> {
        let mut out = Vec::new();
        let mut f = 0;
        loop {
            f = unsafe { EnumClipboardFormats(f) };
            if f == 0 {
                break out;
            }
            out.push(f);
        }
    }

    fn bytes(&self, fmt: u32) -> Option<Vec<u8>> {
        unsafe {
            let h = GetClipboardData(fmt).ok()?;
            global_bytes(HGLOBAL(h.0))
        }
    }
}

/// A copy of an `HGLOBAL`'s contents.
///
/// # Safety
/// `g` must be a valid global memory handle (or null).
pub(crate) unsafe fn global_bytes(g: HGLOBAL) -> Option<Vec<u8>> {
    unsafe {
        let p = GlobalLock(g);
        if p.is_null() {
            return None;
        }
        let len = GlobalSize(g);
        let v = std::slice::from_raw_parts(p as *const u8, len).to_vec();
        let _ = GlobalUnlock(g);
        Some(v)
    }
}

fn empty_read() -> PbRead {
    PbRead { content: None, types: Vec::new(), change_count: 0 }
}

/// Same precedence as macOS: files → URL → image → text. `change_count` is left at 0.
pub(crate) fn read_from(src: &dyn Formats) -> PbRead {
    let fmts = src.list();
    let has = |f: u32| fmts.contains(&f);
    let bytes = |f: u32| if has(f) { src.bytes(f) } else { None };
    let mut types: Vec<String> = fmts.iter().map(|&f| format_name(f)).collect();
    let exclude = registered("ExcludeClipboardContentFromMonitorProcessing");
    let history = registered("CanIncludeInClipboardHistory");
    let ignore = registered("Clipboard Viewer Ignore");
    let no_history = bytes(history).is_some_and(|b| b.get(0..4) == Some(&[0, 0, 0, 0]));
    if has(exclude) || has(ignore) || no_history {
        types.push(TYPE_CONCEALED.into());
    }

    let content = 'found: {
        let files = bytes(CF_HDROP.0 as u32).map(|b| clip::dropfiles_decode(&b)).unwrap_or_default();
        if !files.is_empty() {
            break 'found Some(PbContent::Files(files));
        }
        let url = bytes(registered("UniformResourceLocatorW"))
            .map(|b| clip::utf16_decode(&b).trim().to_owned())
            .filter(|u| !u.is_empty());
        if let Some(url) = url {
            let title = bytes(registered("text/x-moz-url")).and_then(|b| clip::moz_url_decode(&b)).and_then(|(_, t)| t);
            break 'found Some(PbContent::Url { url, title });
        }
        if let Some(png) = image_from(src, &fmts) {
            break 'found Some(PbContent::Image(png));
        }
        let plain = bytes(CF_UNICODETEXT.0 as u32).map(|b| clip::utf16_decode(&b)).filter(|p| !p.trim().is_empty());
        let html = bytes(registered("HTML Format"))
            .and_then(|b| clip::cf_html_decode(&b))
            .filter(|h| !h.trim().is_empty());
        plain.map(|plain| text_content(plain, html))
    };
    PbRead { content, types, change_count: 0 }
}

/// `PNG`, else `CF_DIBV5`/`CF_DIB` converted to PNG.
fn image_from(src: &dyn Formats, fmts: &[u32]) -> Option<Vec<u8>> {
    let png = registered("PNG");
    if fmts.contains(&png) {
        if let Some(png) = src.bytes(png).filter(|b| !b.is_empty()) {
            return Some(png);
        }
    }
    [CF_DIBV5.0 as u32, CF_DIB.0 as u32]
        .into_iter()
        .filter(|f| fmts.contains(f))
        .find_map(|f| src.bytes(f).and_then(|d| clip::dib_to_png(&d)))
}

pub fn change_count(b: Board) -> isize {
    match b {
        Board::General => unsafe { GetClipboardSequenceNumber() as isize },
        Board::Drag => DRAG_COUNT.load(Ordering::Relaxed),
    }
}

/// Reads the board with the macOS precedence; see [`read_from`].
pub fn read(b: Board) -> PbRead {
    let mut r = match b {
        Board::General => match Clipboard::open() {
            Ok(c) => read_from(&c),
            Err(_) => empty_read(),
        },
        Board::Drag => DRAG.with(|d| d.borrow().as_deref().map(read_from)).unwrap_or_else(empty_read),
    };
    r.change_count = change_count(b);
    r
}

/// The board's image as PNG (DIB converted), whatever `read` picked.
pub fn read_image(b: Board) -> Option<Vec<u8>> {
    match b {
        Board::General => {
            let c = Clipboard::open().ok()?;
            image_from(&c, &c.list())
        }
        Board::Drag => DRAG.with(|d| d.borrow().as_deref().and_then(|s| image_from(s, &s.list()))),
    }
}

/// Puts `bytes` on the open clipboard as `fmt`; the clipboard owns the memory afterwards. The block
/// is zeroed, so slack past `bytes` (blocks are rounded up) reads as NULs.
fn set(fmt: u32, bytes: &[u8]) -> AppResult<()> {
    unsafe {
        let g = GlobalAlloc(GHND, bytes.len().max(1)).map_err(|e| AppError::msg(e.to_string()))?;
        let p = GlobalLock(g);
        if p.is_null() {
            let _ = GlobalFree(Some(g));
            return Err(AppError::msg("GlobalLock failed"));
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p as *mut u8, bytes.len());
        let _ = GlobalUnlock(g);
        if let Err(e) = SetClipboardData(fmt, Some(HANDLE(g.0))) {
            let _ = GlobalFree(Some(g));
            return Err(AppError::msg(e.to_string()));
        }
    }
    Ok(())
}

/// NUL-terminated UTF-16LE bytes.
fn utf16z(s: &str) -> Vec<u8> {
    s.encode_utf16().chain(std::iter::once(0)).flat_map(u16::to_le_bytes).collect()
}

/// Replaces the clipboard: files as `CF_HDROP`, the known `data` types as their Windows formats,
/// and optional PNG bytes (also as `CF_DIB`). Concealed/transient data is kept out of clipboard
/// history, cloud clipboard and clipboard monitors.
pub fn write(files: &[PathBuf], data: &BTreeMap<String, String>, png: Option<&[u8]>) -> AppResult<()> {
    if files.is_empty() && data.is_empty() && png.is_none() {
        return Err(AppError::msg("nothing to write to the pasteboard"));
    }
    let _c = Clipboard::open()?;
    unsafe { EmptyClipboard() }.map_err(|e| AppError::msg(e.to_string()))?;
    if !files.is_empty() {
        set(CF_HDROP.0 as u32, &clip::dropfiles_encode(files))?;
    }
    if let Some(t) = data.get(TYPE_PLAIN) {
        set(CF_UNICODETEXT.0 as u32, &utf16z(t))?;
    }
    if let Some(h) = data.get(TYPE_HTML) {
        // NUL-terminated: CF_HTML readers commonly treat the block as a C string.
        set(registered("HTML Format"), &[clip::cf_html_encode(h).as_slice(), &[0]].concat())?;
    }
    if let Some(u) = data.get(TYPE_URL) {
        set(registered("UniformResourceLocatorW"), &utf16z(u))?;
        let moz = match data.get(TYPE_URL_NAME) {
            Some(title) => format!("{u}\n{title}"),
            None => u.clone(),
        };
        set(registered("text/x-moz-url"), &utf16z(&moz))?;
        if !data.contains_key(TYPE_PLAIN) {
            set(CF_UNICODETEXT.0 as u32, &utf16z(u))?;
        }
    }
    if let Some(png) = png {
        set(registered("PNG"), png)?;
        if let Some(dib) = clip::png_to_dib(png) {
            set(CF_DIB.0 as u32, &dib)?;
        }
    }
    if data.contains_key(TYPE_CONCEALED) || data.contains_key(TYPE_TRANSIENT) {
        set(registered("ExcludeClipboardContentFromMonitorProcessing"), &[0])?;
        set(registered("CanIncludeInClipboardHistory"), &0u32.to_le_bytes())?;
        set(registered("CanUploadToCloud"), &0u32.to_le_bytes())?;
    }
    Ok(())
}

pub async fn write_on_main(
    app: &AppHandle,
    files: Vec<PathBuf>,
    data: BTreeMap<String, String>,
    png: Option<Vec<u8>>,
) -> AppResult<()> {
    crate::platform::on_main(app, move |_| write(&files, &data, png.as_deref())).await?
}

/// `text` as plain text marked concealed and transient, so Save clipboard, clipboard history and
/// clipboard managers don't keep it.
fn concealed_text(text: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (TYPE_PLAIN.to_string(), text.to_string()),
        (TYPE_CONCEALED.to_string(), String::new()),
        (TYPE_TRANSIENT.to_string(), String::new()),
    ])
}

/// Copies a secret (the recovery key) to the clipboard; see [`concealed_text`].
pub async fn write_concealed_text(app: &AppHandle, text: String) -> AppResult<()> {
    write_on_main(app, Vec::new(), concealed_text(&text), None).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Touches the real clipboard: run with `cargo test -- --ignored concealed`.
    #[test]
    #[ignore]
    fn concealed_text_is_skipped_by_clipboard_capture() {
        write(&[], &concealed_text("ABCD-EFGH"), None).unwrap();
        let read = read(Board::General);
        // `capture::clipboard::run` skips a board that reads as concealed.
        assert!(read.concealed());
        assert!(matches!(read.content, Some(PbContent::Text { ref plain, .. }) if plain == "ABCD-EFGH"));
    }
}
