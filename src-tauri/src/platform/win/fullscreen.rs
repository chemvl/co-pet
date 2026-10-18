//! Full-screen detection for `platform::fullscreen`.
use windows::Win32::Foundation::RECT;
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetDesktopWindow, GetForegroundWindow, GetShellWindow, GetWindowRect, GetWindowThreadProcessId,
};

use crate::model::Rect;
use crate::platform::MainThread;

const TOLERANCE: f64 = 1.0;

/// True when another app's foreground window covers the pet's monitor (`frame`, physical px).
/// The shell's own full-screen state (`SHQueryUserNotificationState`) only tracks the primary
/// monitor, so the window rect alone decides, as on macOS.
pub fn frontmost_fullscreen_on(_mt: MainThread, frame: Rect) -> bool {
    foreground_covers(frame)
}

fn foreground_covers(frame: Rect) -> bool {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() || hwnd == GetDesktopWindow() || hwnd == GetShellWindow() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == std::process::id() {
            return false;
        }
        // The desktop: `Progman`, or a `WorkerW` holding the icons (wallpaper engines, slideshow).
        let mut class = [0u16; 16];
        let n = GetClassNameW(hwnd, &mut class) as usize;
        if matches!(String::from_utf16_lossy(&class[..n]).as_str(), "WorkerW" | "Progman") {
            return false;
        }
        let mut r = RECT::default();
        if GetWindowRect(hwnd, &mut r).is_err() {
            return false;
        }
        let near = |a: f64, b: f64| (a - b).abs() <= TOLERANCE;
        near(r.left as f64, frame.x)
            && near(r.top as f64, frame.y)
            && near((r.right - r.left) as f64, frame.w)
            && near((r.bottom - r.top) as f64, frame.h)
    }
}
