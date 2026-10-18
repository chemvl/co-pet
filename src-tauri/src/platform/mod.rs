//! OS integration. Shared logic lives in this folder; `macos/` and `win/` implement the OS calls
//! behind identical names. Callers use `crate::platform::<module>`, never a backend path.
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as os;
#[cfg(windows)]
mod win;
#[cfg(windows)]
use win as os;

pub mod calendar;
pub mod clip;
pub mod drag_probe;
pub mod fullscreen;
pub mod idle;
pub mod mic_consent;
pub mod pasteboard;
pub mod pointer;
pub mod screens;
pub mod window_titles;

pub use os::{
    apps, audio_convert, drop_target, frontmost, keys, locale, mic_users, ocr, permissions, pet_window, process,
    screenshot, secret_store, system_audio,
};

use tauri::AppHandle;

use crate::error::{AppError, AppResult};

/// What `apps::picker()` returns: where the excluded-app dialog opens and what it accepts.
pub struct AppPicker {
    pub dir: std::path::PathBuf,
    pub filter_name: &'static str,
    pub extensions: &'static [&'static str],
}

/// Proof of running on the main thread. macOS: `objc2::MainThreadMarker`. Windows: a !Send token
/// that `MainThread::new()` hands out only on the thread that called [`init_main_thread`].
pub use os::MainThread;
#[cfg(windows)]
pub use os::init_main_thread;
/// NUL-terminated UTF-16 for Win32 strings.
#[cfg(windows)]
pub(crate) use os::wide;

/// Runs `f` on the main thread and waits for it, blocking the calling thread. Do not call it from
/// a tokio worker; use [`on_main`] there. OS UI objects are main-thread-only; this runs `f` inline
/// when already on it.
pub fn on_main_sync<R, F>(app: &AppHandle, f: F) -> AppResult<R>
where
    R: Send + 'static,
    F: FnOnce(MainThread) -> R + Send + 'static,
{
    if let Some(mt) = MainThread::new() {
        return Ok(f(mt));
    }
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let mt = MainThread::new().expect("run_on_main_thread runs on the main thread");
        let _ = tx.send(f(mt));
    })?;
    rx.recv().map_err(|_| AppError::msg("main thread task dropped"))
}

/// Runs `f` on the main thread and awaits its result.
pub async fn on_main<R, F>(app: &AppHandle, f: F) -> AppResult<R>
where
    R: Send + 'static,
    F: FnOnce(MainThread) -> R + Send + 'static,
{
    if let Some(mt) = MainThread::new() {
        return Ok(f(mt));
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let mt = MainThread::new().expect("run_on_main_thread runs on the main thread");
        let _ = tx.send(f(mt));
    })?;
    rx.await.map_err(|_| AppError::msg("main thread task dropped"))
}
