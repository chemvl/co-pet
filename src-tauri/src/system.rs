//! App lifecycle: quitting (never leaves `llama-server` or `copet-mcp` behind), open at login,
//! the app appearance, and the run-loop events that decide whether closing the last window quits.
use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, Listener, Manager, RunEvent, Theme};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

use crate::agent::http::McpHttp;
use crate::db::settings;
use crate::events;
use crate::i18n::{self, tr};
use crate::llm::LlmServer;
use crate::transcribe::recorder::Recorder;
use crate::windows;

const OPEN_AT_LOGIN: &str = "general.openAtLogin";
const THEME: &str = "ui.theme";
/// Quit must not hang on a `llama-server` that is still starting.
const STOP_TIMEOUT: Duration = Duration::from_secs(3);

/// Stops a running recording (it is queued and transcribed next launch), the model server and
/// the MCP HTTP server. Used by `quit` and before an update installs.
pub async fn shutdown(app: &AppHandle) {
    if let Some(rec) = app.try_state::<Recorder>() {
        if rec.is_active() {
            if let Err(e) = rec.stop(app).await {
                eprintln!("[system] stopping the recording on quit: {e}");
            }
        }
    }
    if let Some(llm) = app.try_state::<LlmServer>() {
        let _ = tokio::time::timeout(STOP_TIMEOUT, llm.stop()).await;
    }
    if let Some(http) = app.try_state::<McpHttp>() {
        http.stop().await;
    }
}

/// The exit path: `shutdown`, then exit. Signals call it directly, without asking.
pub async fn quit(app: &AppHandle) {
    shutdown(app).await;
    app.exit(0);
}

/// `quit_app` and the menu bar's Quit: quits at once, or asks first while recording.
pub fn request_quit(app: &AppHandle) {
    let recording = app.try_state::<Recorder>().is_some_and(|r| r.is_active());
    if !recording {
        spawn_quit(app.clone());
        return;
    }
    let lang = i18n::lang(app);
    let handle = app.clone();
    app.dialog()
        .message(tr(lang, "quit.recording.body"))
        .title(tr(lang, "quit.recording.title"))
        .buttons(MessageDialogButtons::OkCancelCustom(
            tr(lang, "quit.recording.ok").into(),
            tr(lang, "quit.recording.cancel").into(),
        ))
        .show(move |ok| {
            if ok {
                spawn_quit(handle);
            }
        });
}

fn spawn_quit(app: AppHandle) {
    tauri::async_runtime::spawn(async move { quit(&app).await });
}

/// SIGTERM/SIGINT/SIGHUP (macOS) or console close/logoff/shutdown (Windows) exit through `quit`
/// instead of ending the process outright, which would orphan `llama-server`.
pub fn quit_on_signals(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        signals().await;
        quit(&app).await;
    });
}

/// Resolves on the first quit signal; never when the handlers can't be installed.
#[cfg(unix)]
async fn signals() {
    use tokio::signal::unix::{signal, SignalKind};
    let (Ok(mut term), Ok(mut int), Ok(mut hup)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
        signal(SignalKind::hangup()),
    ) else {
        eprintln!("[system] could not install signal handlers");
        return std::future::pending().await;
    };
    tokio::select! {
        _ = term.recv() => {}
        _ = int.recv() => {}
        _ = hup.recv() => {}
    }
}

/// Resolves on the first console control event; never when the handlers can't be installed.
#[cfg(windows)]
async fn signals() {
    use tokio::signal::windows::{ctrl_break, ctrl_c, ctrl_close, ctrl_logoff, ctrl_shutdown};
    let (Ok(mut c), Ok(mut b), Ok(mut close), Ok(mut logoff), Ok(mut shutdown)) =
        (ctrl_c(), ctrl_break(), ctrl_close(), ctrl_logoff(), ctrl_shutdown())
    else {
        eprintln!("[system] could not install console handlers");
        return std::future::pending().await;
    };
    tokio::select! {
        _ = c.recv() => {}
        _ = b.recv() => {}
        _ = close.recv() => {}
        _ = logoff.recv() => {}
        _ = shutdown.recv() => {}
    }
}

/// Matches the login item to `general.openAtLogin`. Debug builds only log: a login item pointing
/// at `target/debug` would start a dev binary without its dev server at every login.
pub fn apply_autostart(app: &AppHandle) {
    let want = settings::read::<bool>(app, OPEN_AT_LOGIN).unwrap_or(true);
    if cfg!(debug_assertions) {
        eprintln!("[autostart] debug build: login item left as is (setting says {want})");
        return;
    }
    let launcher = app.autolaunch();
    if launcher.is_enabled().is_ok_and(|on| on == want) {
        return;
    }
    let result = if want { launcher.enable() } else { launcher.disable() };
    if let Err(e) = result {
        eprintln!("[autostart] {}: {e}", if want { "enable" } else { "disable" });
    }
}

/// Re-applies the login item when the setting changes (after onboarding has finished; onboarding
/// applies it on Finish).
pub fn watch_autostart(app: &AppHandle) {
    let handle = app.clone();
    app.listen_any(events::SETTINGS_CHANGED, move |event| {
        let key = serde_json::from_str::<Value>(event.payload()).ok();
        if key.as_ref().and_then(|v| v["key"].as_str()) != Some(OPEN_AT_LOGIN) {
            return;
        }
        if settings::read::<bool>(&handle, "onboarding.completed").unwrap_or(false) {
            apply_autostart(&handle);
        }
    });
}

/// `ui.theme` as the app appearance: `light` / `dark` override macOS, `system` follows it. Every
/// webview's `prefers-color-scheme` (and so `<html data-theme>`) and the native chrome follow it.
fn apply_theme(app: &AppHandle) {
    let theme = match settings::read::<String>(app, THEME).as_deref() {
        Ok("light") => Some(Theme::Light),
        Ok("dark") => Some(Theme::Dark),
        _ => None,
    };
    app.set_theme(theme);
}

/// Applies `ui.theme` now (before any window opens) and whenever it changes.
pub fn follow_theme(app: &AppHandle) {
    apply_theme(app);
    let handle = app.clone();
    app.listen_any(events::SETTINGS_CHANGED, move |event| {
        let key = serde_json::from_str::<Value>(event.payload()).ok();
        if key.as_ref().and_then(|v| v["key"].as_str()) == Some(THEME) {
            apply_theme(&handle);
        }
    });
}

/// `App::run` callback. Closing Home keeps Co-pet running while the pet exists (during
/// onboarding, closing the window quits); every exit path stops `llama-server` and the MCP HTTP
/// server first, because `exit` skips destructors and `kill_on_drop` would never run.
pub fn on_run_event(app: &AppHandle, event: RunEvent) {
    match event {
        RunEvent::ExitRequested { code: None, api, .. } if windows::pet_window(app).is_some() => {
            api.prevent_exit();
        }
        RunEvent::Exit => {
            if let Some(llm) = app.try_state::<LlmServer>() {
                // The timer must be created inside the runtime, so it lives in the async block.
                tauri::async_runtime::block_on(async move {
                    let _ = tokio::time::timeout(STOP_TIMEOUT, llm.stop()).await;
                });
            }
            if let Some(http) = app.try_state::<McpHttp>() {
                tauri::async_runtime::block_on(http.stop());
            }
        }
        _ => {}
    }
}
