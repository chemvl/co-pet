//! Windows half of `platform::screens`. Global units are physical pixels (Tauri runs per-monitor
//! DPI aware v2), top-left origin at the primary monitor.
use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, MONITORINFOF_PRIMARY};

use crate::model::Rect;
use crate::platform::screens::Screen;
use crate::platform::MainThread;

fn rect(r: RECT) -> Rect {
    Rect { x: r.left as f64, y: r.top as f64, w: (r.right - r.left) as f64, h: (r.bottom - r.top) as f64 }
}

unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> BOOL {
    let out = &mut *(lp.0 as *mut Vec<(bool, Screen)>);
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if !GetMonitorInfoW(m, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool() {
        return true.into();
    }
    let (mut dx, mut dy) = (96u32, 96u32);
    let _ = GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
    let name = super::from_wide(&info.szDevice);
    let r = info.monitorInfo.rcMonitor;
    out.push((
        info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
        Screen {
            id: format!("{name}:{}x{}@{},{}", r.right - r.left, r.bottom - r.top, r.left, r.top),
            name,
            frame: rect(r),
            visible: rect(info.monitorInfo.rcWork),
            scale: f64::from(dx) / 96.0,
        },
    ));
    true.into()
}

/// Every monitor, primary first (macOS orders `NSScreen::screens` the same way).
pub fn screens(_mt: MainThread) -> Vec<Screen> {
    let mut all: Vec<(bool, Screen)> = Vec::new();
    let _ = unsafe { EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut all as *mut _ as isize)) };
    all.sort_by_key(|(primary, _)| !primary);
    all.into_iter().map(|(_, s)| s).collect()
}

pub fn mouse_location(_mt: MainThread) -> (f64, f64) {
    let mut p = POINT::default();
    let _ = unsafe { GetCursorPos(&mut p) };
    (p.x as f64, p.y as f64)
}
