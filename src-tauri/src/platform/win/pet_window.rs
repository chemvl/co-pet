//! The pet window on Windows: a transparent, undecorated, always-on-top Tauri window that never
//! activates (`WS_EX_NOACTIVATE`) and stays out of the taskbar and Alt+Tab (`WS_EX_TOOLWINDOW`).
//!
//! tao owns `GWL_EXSTYLE`: every window-flag change (`set_focusable`, `set_ignore_cursor_events`,
//! `show`) rewrites it from tao's own flags. `WS_EX_NOACTIVATE` is therefore tao's `focusable`
//! flag, and `WS_EX_TOOLWINDOW`, which tao has no flag for, is re-added after each of those calls.
//! They run synchronously only on the main thread, so `focus`/`resign` hop there.
use std::sync::atomic::{AtomicIsize, Ordering};

use tauri::{AppHandle, WebviewWindow, WebviewWindowBuilder, Wry};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
    GWL_EXSTYLE, HWND_TOPMOST, SWP_NOACTIVATE, WS_EX_TOOLWINDOW,
};

use crate::error::{AppError, AppResult};
use crate::model::Rect;
use crate::platform::{on_main_sync, MainThread};

/// The window that had the foreground before `focus` (restored by `resign`).
static PREVIOUS: AtomicIsize = AtomicIsize::new(0);

fn hwnd(w: &WebviewWindow) -> Option<HWND> {
    w.hwnd().ok()
}

/// Re-adds `WS_EX_TOOLWINDOW` after tao rewrote the extended style.
fn keep_tool_window(w: &WebviewWindow) {
    let Some(h) = hwnd(w) else { return };
    let tool = WS_EX_TOOLWINDOW.0 as isize;
    unsafe {
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        if ex & tool == 0 {
            SetWindowLongPtrW(h, GWL_EXSTYLE, ex | tool);
        }
    }
}

/// Nothing to register: the pet window is a plain Tauri window (D2).
pub fn plugin(b: tauri::Builder<Wry>) -> tauri::Builder<Wry> {
    b
}

/// Not focusable: tao gives the window `WS_EX_NOACTIVATE`.
pub fn builder<'a>(b: WebviewWindowBuilder<'a, Wry, AppHandle>) -> WebviewWindowBuilder<'a, Wry, AppHandle> {
    b.focusable(false)
}

pub fn configure(app: &AppHandle, mt: MainThread, w: &WebviewWindow) -> AppResult<()> {
    hwnd(w).ok_or_else(|| AppError::msg("pet window: no HWND"))?;
    // Also adds `WS_EX_TOOLWINDOW`.
    set_ignores_mouse(mt, w, true);
    super::drop_target::install(app, mt);
    Ok(())
}

pub fn frame(_mt: MainThread, w: &WebviewWindow) -> Rect {
    let mut r = Default::default();
    match hwnd(w) {
        Some(h) if unsafe { GetWindowRect(h, &mut r) }.is_ok() => {
            Rect { x: r.left as f64, y: r.top as f64, w: (r.right - r.left) as f64, h: (r.bottom - r.top) as f64 }
        }
        _ => Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 },
    }
}

/// Re-asserts `HWND_TOPMOST` on each placement.
pub fn set_frame(_mt: MainThread, w: &WebviewWindow, f: Rect) {
    if let Some(h) = hwnd(w) {
        let _ = unsafe {
            SetWindowPos(h, Some(HWND_TOPMOST), f.x as i32, f.y as i32, f.w as i32, f.h as i32, SWP_NOACTIVATE)
        };
    }
}

pub fn set_ignores_mouse(_mt: MainThread, w: &WebviewWindow, ignore: bool) {
    if let Err(e) = w.set_ignore_cursor_events(ignore) {
        eprintln!("[pet] ignore cursor: {e}");
    }
    keep_tool_window(w);
}

/// Through tao, so its visible flag is set; a later flag change would hide the window otherwise.
pub fn show(_mt: MainThread, w: &WebviewWindow) {
    if let Err(e) = w.show() {
        eprintln!("[pet] show: {e}");
    }
    keep_tool_window(w);
}

/// Typing goes to the pet: make it focusable and take the foreground (allowed right after a
/// global hotkey or a click on the pet).
pub fn focus(app: &AppHandle, w: &WebviewWindow) -> AppResult<()> {
    let w = w.clone();
    on_main_sync(app, move |_| -> AppResult<()> {
        let h = hwnd(&w).ok_or_else(|| AppError::msg("no pet window"))?;
        let prev = unsafe { GetForegroundWindow() };
        if prev != h {
            PREVIOUS.store(prev.0 as isize, Ordering::Relaxed);
        }
        w.set_focusable(true)?;
        keep_tool_window(&w);
        let _ = unsafe { SetForegroundWindow(h) };
        Ok(())
    })?
}

/// Gives the keyboard back to the window the user was typing in.
pub fn resign(app: &AppHandle, w: &WebviewWindow) -> AppResult<()> {
    let w = w.clone();
    on_main_sync(app, move |_| -> AppResult<()> {
        let h = hwnd(&w).ok_or_else(|| AppError::msg("no pet window"))?;
        w.set_focusable(false)?;
        keep_tool_window(&w);
        let prev = HWND(PREVIOUS.swap(0, Ordering::Relaxed) as *mut _);
        if !prev.is_invalid() && unsafe { GetForegroundWindow() } == h {
            let _ = unsafe { SetForegroundWindow(prev) };
        }
        Ok(())
    })?
}

/// Home and Onboarding draw their own 44 px title bar with caption buttons on the right.
pub fn overlay<'a>(b: WebviewWindowBuilder<'a, Wry, AppHandle>) -> WebviewWindowBuilder<'a, Wry, AppHandle> {
    b.decorations(false).shadow(true)
}

pub fn bring_to_front(w: &WebviewWindow) {
    let _ = w.set_focus();
    if let Some(h) = hwnd(w) {
        let _ = unsafe { SetForegroundWindow(h) };
    }
}

/// Windows always lists Home/Onboarding in the taskbar; nothing to switch.
pub fn set_in_task_switcher(_app: &AppHandle, _visible: bool) -> AppResult<()> {
    Ok(())
}
