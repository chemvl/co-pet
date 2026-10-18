//! Installed apps: the picker location, an app's id (lowercase exe name) and its display name.
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};

use crate::platform::AppPicker;

/// Where and what the "exclude an app" dialog picks: `.exe` files under Program Files.
pub fn picker() -> AppPicker {
    AppPicker {
        dir: std::env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files")),
        filter_name: "Applications",
        extensions: &["exe"],
    }
}

/// The app's id (bundle id on macOS, lowercase exe name on Windows); `None` when `path` isn't an
/// `.exe`.
pub fn app_id(path: &Path) -> Option<String> {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
        .then(|| super::app_id_of(path))
        .flatten()
}

/// `FileDescription` of a running process with that exe name, else the name without `.exe`.
/// One Toolhelp snapshot, no per-process queries: the excluded-apps list calls this per app.
pub fn display_name(app_id: &str) -> Option<String> {
    running_pid(app_id)
        .and_then(super::exe_path)
        .and_then(|p| super::exe_description(&p))
        .or_else(|| Some(app_id.trim_end_matches(".exe").to_owned()))
}

/// A running process whose exe file name is `exe` (case-insensitive).
fn running_pid(exe: &str) -> Option<u32> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut e = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = None;
        let mut more = Process32FirstW(snap, &mut e).is_ok();
        while more {
            if super::from_wide(&e.szExeFile).eq_ignore_ascii_case(exe) {
                found = Some(e.th32ProcessID);
                break;
            }
            more = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
        found
    }
}
