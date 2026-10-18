//! Native drop target (D3). A borderless layered tool window (alpha 1, so it is hit-testable but
//! invisible) is shown over the pet and its halo only while an OS drag is in progress, and is
//! registered with `RegisterDragDrop`. On drop, the data object becomes `Board::Drag` for the
//! duration of `capture::drop::read`, exactly like the macOS drag pasteboard.
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use windows::core::{implement, Ref, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINTL, WPARAM};
use windows::Win32::System::Com::{CoTaskMemFree, IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::{
    IDropTarget, IDropTarget_Impl, OleInitialize, RegisterDragDrop, ReleaseStgMedium, DROPEFFECT, DROPEFFECT_COPY,
    DROPEFFECT_NONE,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, SetLayeredWindowAttributes, SetWindowPos, ShowWindow,
    HWND_TOPMOST, LWA_ALPHA, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use super::pasteboard::{global_bytes, set_drag_source, Formats};
use crate::model::Rect;
use crate::platform::{frontmost, pointer, MainThread};

const OWN_MARK_WINDOW_MS: u64 = 3_000;
static OWN: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static CATCHER: Cell<Option<HWND>> = const { Cell::new(None) };
    static SOURCE: RefCell<Option<frontmost::FrontApp>> = const { RefCell::new(None) };
    static ACCEPT: Cell<bool> = const { Cell::new(false) };
    /// The catcher's last placement in px (`None` = hidden); see `place_catcher`.
    static PLACED: Cell<Option<(i32, i32, i32, i32)>> = const { Cell::new(None) };
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// A drag that Co-pet itself starts (drag-out) must not land back on the pet.
pub fn mark_own_drag() {
    OWN.store(now_ms(), Ordering::Relaxed);
}

struct DataObjectFormats(IDataObject);

impl Formats for DataObjectFormats {
    fn list(&self) -> Vec<u32> {
        let mut out = Vec::new();
        unsafe {
            if let Ok(e) = self.0.EnumFormatEtc(1 /* DATADIR_GET */) {
                let mut f = [FORMATETC::default()];
                let mut n = 0u32;
                while e.Next(&mut f, Some(&mut n as *mut u32)).is_ok() && n == 1 {
                    out.push(u32::from(f[0].cfFormat));
                    if !f[0].ptd.is_null() {
                        CoTaskMemFree(Some(f[0].ptd as *const _));
                    }
                }
            }
        }
        out
    }

    fn bytes(&self, fmt: u32) -> Option<Vec<u8>> {
        let fe = FORMATETC {
            cfFormat: fmt as u16,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        unsafe {
            let mut medium = self.0.GetData(&fe).ok()?;
            let bytes = if medium.tymed == TYMED_HGLOBAL.0 as u32 { global_bytes(medium.u.hGlobal) } else { None };
            ReleaseStgMedium(&mut medium);
            bytes
        }
    }
}

/// `Copy` while the drag is usable and the cursor (screen px) is inside the pet's drop halo, like
/// macOS; the catcher is the halo's bounding box, so its corners may lie outside the halo.
fn effect_at(pt: &POINTL) -> DROPEFFECT {
    let in_halo = || {
        let (Some(app), Some(mt)) = (APP.get(), MainThread::new()) else { return false };
        pointer::in_drop_halo(app, mt, f64::from(pt.x), f64::from(pt.y))
    };
    if ACCEPT.get() && in_halo() {
        DROPEFFECT_COPY
    } else {
        DROPEFFECT_NONE
    }
}

#[implement(IDropTarget)]
struct Target;

impl IDropTarget_Impl for Target_Impl {
    fn DragEnter(
        &self,
        data: Ref<IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let own = now_ms().saturating_sub(OWN.load(Ordering::Relaxed)) < OWN_MARK_WINDOW_MS;
        let usable = !own
            && data
                .as_ref()
                .is_some_and(|d| super::pasteboard::read_from(&DataObjectFormats(d.clone())).content.is_some());
        ACCEPT.set(usable);
        // The pet never activates, so the foreground app is the drag source.
        SOURCE.with(|s| *s.borrow_mut() = usable.then(frontmost::frontmost_app).flatten());
        unsafe { *effect = effect_at(pt) };
        Ok(())
    }

    fn DragOver(&self, _keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        unsafe { *effect = effect_at(pt) };
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        ACCEPT.set(false);
        Ok(())
    }

    fn Drop(
        &self,
        data: Ref<IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        // Never leave the source's allowed effects (which may include MOVE) in place.
        unsafe { *effect = DROPEFFECT_NONE };
        let Some(app) = APP.get() else { return Ok(()) };
        let source = SOURCE.with(|s| s.borrow_mut().take());
        let dropped = data.as_ref().and_then(|d| {
            set_drag_source(Some(Box::new(DataObjectFormats(d.clone()))));
            let r = crate::capture::drop::read(source);
            set_drag_source(None);
            r
        });
        unsafe { *effect = if dropped.is_some() { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        if let Some(d) = dropped {
            crate::capture::drop::spawn(app, d);
        }
        ACCEPT.set(false);
        Ok(())
    }
}

static APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

unsafe extern "system" fn wndproc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    DefWindowProcW(h, m, w, l)
}

/// Creates the hidden catcher and registers it as a drop target (once, main thread).
pub fn install(app: &tauri::AppHandle, _mt: MainThread) {
    let _ = APP.set(app.clone());
    if CATCHER.get().is_some() {
        return;
    }
    unsafe {
        let _ = OleInitialize(None);
        let instance: HINSTANCE = GetModuleHandleW(None).map(Into::into).unwrap_or_default();
        let class = crate::platform::wide("CopetDropCatcher");
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        });
        let Ok(h) = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
            PCWSTR(class.as_ptr()),
            PCWSTR::null(),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(instance),
            None,
        ) else {
            eprintln!("[drop] catcher window failed");
            return;
        };
        let _ = SetLayeredWindowAttributes(h, COLORREF(0), 1, LWA_ALPHA);
        let target: IDropTarget = Target.into();
        if let Err(e) = RegisterDragDrop(h, &target) {
            // A visible catcher without a drop target would only refuse every drop in the halo.
            eprintln!("[drop] RegisterDragDrop: {e}");
            let _ = DestroyWindow(h);
            return;
        }
        CATCHER.set(Some(h));
    }
}

/// Shows the catcher over `rect` (global px, already covering pet + halo) or hides it. Only a
/// change calls into Win32: the pet's own placement (`pet_window::set_frame`, which re-asserts
/// `HWND_TOPMOST`) runs on monitor changes, which also move this rect, so the catcher goes back
/// above the pet then; tao's click-through toggling uses `SWP_NOZORDER`.
pub fn place_catcher(_mt: MainThread, rect: Option<Rect>) {
    let Some(h) = CATCHER.get() else { return };
    let px = rect.map(|r| (r.x as i32, r.y as i32, r.w as i32, r.h as i32));
    if PLACED.replace(px) == px {
        return;
    }
    unsafe {
        match px {
            Some((x, y, w, ht)) => {
                let _ = SetWindowPos(h, Some(HWND_TOPMOST), x, y, w, ht, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            }
            None => {
                let _ = ShowWindow(h, SW_HIDE);
            }
        }
    }
}
