//! Window titles and running apps for `platform::window_titles`. Titles are read without a
//! permission on Windows.
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowLongW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    GWL_EXSTYLE, WS_EX_TOOLWINDOW,
};

use crate::platform::window_titles::TitleSource;

/// Window titles need no permission on Windows.
pub fn title_source() -> TitleSource {
    TitleSource::Native
}

struct Collect<'a> {
    pids: &'a [i32],
    out: Vec<(i32, String)>,
}

unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> BOOL {
    let c = &mut *(lp.0 as *mut Collect);
    if !IsWindowVisible(hwnd).as_bool() || GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
        return true.into();
    }
    let mut cloaked = 0u32;
    let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&mut cloaked as *mut u32).cast(), 4);
    if cloaked != 0 {
        return true.into();
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if !c.pids.contains(&(pid as i32)) {
        return true.into();
    }
    let len = GetWindowTextLengthW(hwnd);
    if len > 0 {
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, &mut buf);
        let title = String::from_utf16_lossy(&buf[..n as usize]);
        if !title.trim().is_empty() {
            c.out.push((pid as i32, title));
        }
    }
    true.into()
}

/// Titles of the visible, uncloaked top-level windows owned by `pids`, in z-order.
pub fn titles_for(pids: &[i32]) -> Vec<(i32, String)> {
    let mut c = Collect { pids, out: Vec::new() };
    let _ = unsafe { EnumWindows(Some(each), LPARAM(&mut c as *mut Collect as isize)) };
    c.out
}

/// (pid, lowercase exe name) of every process.
pub fn running_apps() -> Vec<(i32, String)> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    sys.processes().values().map(|p| (p.pid().as_u32() as i32, p.name().to_string_lossy().to_lowercase())).collect()
}
