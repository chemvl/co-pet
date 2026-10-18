//! Windows integration. Same public names as `platform::macos`; see `platform/mod.rs`.
pub mod apps;
pub mod audio_convert;
pub mod calendar;
pub mod drag_out;
pub mod drop_target;
pub mod frontmost;
pub mod fullscreen;
pub mod idle;
pub mod keys;
pub mod locale;
pub mod mic_users;
pub mod ocr;
pub mod pasteboard;
pub mod permissions;
pub mod pet_window;
pub mod pointer;
pub mod process;
pub mod screens;
pub mod screenshot;
pub mod secret_store;
pub mod system_audio;
pub mod window_titles;

use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::thread::ThreadId;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};

static MAIN: OnceLock<ThreadId> = OnceLock::new();

/// Records the calling thread as the main thread. First line of `lib::run()`.
pub fn init_main_thread() {
    let _ = MAIN.set(std::thread::current().id());
}

/// Proof of running on the main (event-loop) thread; deliberately `!Send`.
#[derive(Clone, Copy, Debug)]
pub struct MainThread(PhantomData<*const ()>);

impl MainThread {
    pub fn new() -> Option<Self> {
        (MAIN.get() == Some(&std::thread::current().id())).then_some(MainThread(PhantomData))
    }
}

/// NUL-terminated UTF-16.
pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// UTF-16 up to the first NUL.
pub(crate) fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Full image path of a process, if this user may query it.
pub(crate) fn exe_path(pid: u32) -> Option<PathBuf> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        ok.then(|| PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])))
    }
}

/// The exe's `FileDescription` (e.g. "Google Chrome"), else its file stem.
pub(crate) fn exe_description(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let w = wide(&path.to_string_lossy());
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(w.as_ptr()), None);
        if size == 0 {
            return Some(stem);
        }
        let mut data = vec![0u8; size as usize];
        if GetFileVersionInfoW(PCWSTR(w.as_ptr()), None, size, data.as_mut_ptr().cast()).is_err() {
            return Some(stem);
        }
        let mut ptr = std::ptr::null_mut();
        let mut len = 0u32;
        let tr = wide(r"\VarFileInfo\Translation");
        if !VerQueryValueW(data.as_ptr().cast(), PCWSTR(tr.as_ptr()), &mut ptr, &mut len).as_bool() || len < 4 {
            return Some(stem);
        }
        // Resource data in the version block carries no alignment guarantee for `u16`.
        let lang = (ptr as *const u16).read_unaligned();
        let cp = (ptr as *const u16).add(1).read_unaligned();
        let key = wide(&format!(r"\StringFileInfo\{lang:04x}{cp:04x}\FileDescription"));
        if VerQueryValueW(data.as_ptr().cast(), PCWSTR(key.as_ptr()), &mut ptr, &mut len).as_bool() && len > 1 {
            let units: Vec<u16> = (0..len as usize).map(|i| (ptr as *const u16).add(i).read_unaligned()).collect();
            let s = from_wide(&units);
            if !s.trim().is_empty() {
                return Some(s.trim().to_owned());
            }
        }
        Some(stem)
    }
}

/// `bundle_id` on Windows: the lowercase exe file name.
pub(crate) fn app_id_of(path: &Path) -> Option<String> {
    Some(path.file_name()?.to_string_lossy().to_lowercase())
}
