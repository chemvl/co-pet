//! Tray drag-out on Windows. tauri-plugin-drag can't be used here: its Windows backend drags files
//! only, and turns typed data (a text or link item) into a dummy drag that is cancelled on drop.
//! This drags a shell data object carrying the same formats as Copy (`pasteboard::formats`), so
//! text lands as text and HTML, links as URLs, files as `CF_HDROP`, with the card as drag image.
use std::collections::BTreeMap;
use std::mem::ManuallyDrop;
use std::path::PathBuf;

use windows::core::{implement, HRESULT};
use windows::Win32::Foundation::{
    COLORREF, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, POINT, S_OK, SIZE,
};
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
use windows::Win32::System::Com::{
    CoCreateInstance, IDataObject, CLSCTX_INPROC_SERVER, DVASPECT_CONTENT, FORMATETC, STGMEDIUM, STGMEDIUM_0,
    TYMED_HGLOBAL,
};
use windows::Win32::System::Ole::{DoDragDrop, IDropSource, IDropSource_Impl, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::{CLSID_DragDropHelper, IDragSourceHelper, SHCreateDataObject, SHDRAGIMAGE};

use super::pasteboard::{formats, global_alloc};
use crate::error::{AppError, AppResult};
use crate::platform::MainThread;

/// Pixels per CSS px of the drag image (`SCALE` in `src/pet/dragImage.ts`).
const IMAGE_SCALE: f64 = 2.0;
/// Where the cursor holds the card, in CSS px of the image: the left part of the card, mid-height
/// (`PAD + 24`, `PAD + CARD_H / 2` in `dragImage.ts`).
const GRIP: (f64, f64) = (48.0, 56.0);

#[implement(IDropSource)]
struct Source;

impl IDropSource_Impl for Source_Impl {
    fn QueryContinueDrag(&self, escape: windows::core::BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape.as_bool() {
            DRAGDROP_S_CANCEL
        } else if (keys & MK_LBUTTON).0 == 0 {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// The data object for a drag: every format of [`formats`] as an `HGLOBAL`.
pub(crate) fn data_object(files: &[PathBuf], data: &BTreeMap<String, String>) -> AppResult<IDataObject> {
    let obj: IDataObject =
        unsafe { SHCreateDataObject(None, None, None) }.map_err(|e| AppError::msg(format!("data object: {e}")))?;
    for (fmt, bytes) in formats(files, data, None) {
        let fe = FORMATETC {
            cfFormat: fmt as u16,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        let medium = STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: global_alloc(&bytes)? },
            pUnkForRelease: ManuallyDrop::new(None),
        };
        // `true`: the data object owns (and frees) the block from here on.
        unsafe { obj.SetData(&fe, &medium, true) }.map_err(|e| AppError::msg(format!("set format {fmt}: {e}")))?;
    }
    Ok(obj)
}

/// Runs the OS drag until the drop or cancel; `true` when a target accepted the drop. `icon` is
/// the card PNG; `scale` the pet window's pixels per CSS px. Blocks the main thread in OLE's modal
/// loop, which keeps dispatching window messages.
pub fn run(_mt: MainThread, files: &[PathBuf], data: &BTreeMap<String, String>, icon: &[u8], scale: f64) -> AppResult<bool> {
    let obj = data_object(files, data)?;
    if let Err(e) = set_drag_image(&obj, icon, scale) {
        // The drag still works with the system's default image.
        eprintln!("[drag] image: {e}");
    }
    let source: IDropSource = Source.into();
    let mut effect = DROPEFFECT_NONE;
    let hr = unsafe { DoDragDrop(&obj, &source, DROPEFFECT_COPY, &mut effect) };
    if hr.is_err() {
        return Err(AppError::msg(format!("DoDragDrop: {hr:?}")));
    }
    Ok(hr == DRAGDROP_S_DROP && effect != DROPEFFECT_NONE)
}

/// Decodes the card PNG, resizes it to the window's pixel scale and hands it to the shell's drag
/// image helper, which stores it in `obj`.
fn set_drag_image(obj: &IDataObject, icon: &[u8], scale: f64) -> AppResult<()> {
    let img = image::load_from_memory_with_format(icon, image::ImageFormat::Png)
        .map_err(|e| AppError::msg(e.to_string()))?
        .into_rgba8();
    let k = scale / IMAGE_SCALE;
    let img = if (k - 1.0).abs() < 0.01 {
        img
    } else {
        let w = ((f64::from(img.width()) * k).round() as u32).max(1);
        let h = ((f64::from(img.height()) * k).round() as u32).max(1);
        image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle)
    };
    let (w, h) = img.dimensions();
    // Top-down BGRA with premultiplied alpha, as the shell expects for a 32-bit drag image.
    let mut bits = img.into_raw();
    for px in bits.as_chunks_mut::<4>().0 {
        let a = u16::from(px[3]);
        let premul = |c: u8| ((u16::from(c) * a + 127) / 255) as u8;
        *px = [premul(px[2]), premul(px[1]), premul(px[0]), px[3]];
    }
    unsafe {
        let bmp = CreateBitmap(w as i32, h as i32, 1, 32, Some(bits.as_ptr().cast()));
        if bmp.is_invalid() {
            return Err(AppError::msg("CreateBitmap failed"));
        }
        let shdi = SHDRAGIMAGE {
            sizeDragImage: SIZE { cx: w as i32, cy: h as i32 },
            ptOffset: POINT { x: (GRIP.0 * scale).round() as i32, y: (GRIP.1 * scale).round() as i32 },
            hbmpDragImage: bmp,
            crColorKey: COLORREF(0xFFFF_FFFF),
        };
        let helper: IDragSourceHelper = CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| AppError::msg(e.to_string()))?;
        // On success the helper owns the bitmap.
        if let Err(e) = helper.InitializeFromBitmap(&shdi, obj) {
            let _ = DeleteObject(bmp.into());
            return Err(AppError::msg(e.to_string()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::pasteboard::{PbContent, TYPE_HTML, TYPE_PLAIN, TYPE_URL, TYPE_URL_NAME};

    fn ole() {
        let _ = unsafe { windows::Win32::System::Ole::OleInitialize(None) };
    }

    fn read(obj: &IDataObject) -> Option<PbContent> {
        super::super::drop_target::read_data_object(obj).content
    }

    #[test]
    fn a_text_drag_reads_back_as_text_and_html() {
        ole();
        let data = BTreeMap::from([
            (TYPE_PLAIN.to_string(), "Ghi chú: họp 14:00".to_string()),
            (TYPE_HTML.to_string(), "<b>Ghi chú</b>".to_string()),
        ]);
        let obj = data_object(&[], &data).unwrap();
        assert_eq!(
            read(&obj),
            Some(PbContent::Text { plain: "Ghi chú: họp 14:00".into(), html: Some("<b>Ghi chú</b>".into()) })
        );
    }

    #[test]
    fn a_link_drag_reads_back_as_the_url() {
        ole();
        let data = BTreeMap::from([
            (TYPE_URL.to_string(), "https://example.com/a?b=1".to_string()),
            (TYPE_URL_NAME.to_string(), "Example".to_string()),
            (TYPE_PLAIN.to_string(), "https://example.com/a?b=1".to_string()),
        ]);
        let obj = data_object(&[], &data).unwrap();
        assert!(matches!(read(&obj), Some(PbContent::Url { url, .. }) if url == "https://example.com/a?b=1"));
    }

    #[test]
    fn a_file_drag_reads_back_as_the_file() {
        ole();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.md");
        std::fs::write(&file, "hi").unwrap();
        let obj = data_object(std::slice::from_ref(&file), &BTreeMap::new()).unwrap();
        assert_eq!(read(&obj), Some(PbContent::Files(vec![file])));
    }
}
