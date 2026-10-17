//! macOS integration. AppKit objects are main-thread-only: callers off the main thread go through
//! [`on_main`] (async) or [`on_main_sync`] (blocking), which run inline when already on it.
pub mod calendar;
pub mod drop_target;
pub mod frontmost;
pub mod fullscreen;
pub mod idle;
pub mod keys;
pub mod pasteboard;
pub mod permissions;
pub mod pointer;
pub mod screens;

use objc2::MainThreadMarker;
use tauri::AppHandle;

use crate::error::{AppError, AppResult};

/// Runs `f` on the main thread and waits for it, blocking the calling thread. Do not call it from
/// a tokio worker; use [`on_main`] there.
pub fn on_main_sync<R, F>(app: &AppHandle, f: F) -> AppResult<R>
where
    R: Send + 'static,
    F: FnOnce(MainThreadMarker) -> R + Send + 'static,
{
    if let Some(mtm) = MainThreadMarker::new() {
        return Ok(f(mtm));
    }
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let mtm = MainThreadMarker::new().expect("run_on_main_thread runs on the main thread");
        let _ = tx.send(f(mtm));
    })?;
    rx.recv().map_err(|_| AppError::msg("main thread task dropped"))
}

/// Runs `f` on the main thread and awaits its result.
pub async fn on_main<R, F>(app: &AppHandle, f: F) -> AppResult<R>
where
    R: Send + 'static,
    F: FnOnce(MainThreadMarker) -> R + Send + 'static,
{
    if let Some(mtm) = MainThreadMarker::new() {
        return Ok(f(mtm));
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let mtm = MainThreadMarker::new().expect("run_on_main_thread runs on the main thread");
        let _ = tx.send(f(mtm));
    })?;
    rx.await.map_err(|_| AppError::msg("main thread task dropped"))
}
