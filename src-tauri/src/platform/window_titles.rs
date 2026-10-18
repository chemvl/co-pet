//! Call detection signals besides the microphone: window titles of given processes (Zoom's
//! meeting window, a browser's Meet tab), the running apps' ids (bundle id on macOS, lowercase exe
//! name on Windows), and whether a process with a given name runs (Zoom's meeting helper).
//!
//! Titles are matched in memory by the caller and never logged.
use std::ffi::OsStr;

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

pub use super::os::window_titles::{running_apps, title_source, titles_for};

/// Where window titles can come from on this computer right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleSource {
    /// macOS window server (needs Screen Recording).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    ScreenRecording,
    /// macOS Accessibility.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Accessibility,
    /// Read without a permission (Windows).
    #[cfg_attr(not(windows), allow(dead_code))]
    Native,
    /// macOS without either permission.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    None,
}

/// Whether a process whose name is exactly one of `names` runs.
pub fn process_running(names: &[&str]) -> bool {
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    sys.processes().values().any(|p| names.iter().any(|n| p.name() == OsStr::new(n)))
}
