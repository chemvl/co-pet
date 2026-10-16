//! App lifecycle: quitting (never leaves `llama-server` or `copet-mcp` behind), open at login,
//! and the run-loop events that decide whether closing the last window quits.
use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, Listener, Manager, RunEvent};
use tauri_plugin_autostart::ManagerExt as _;

use crate::agent::http::McpHttp;
use crate::db::settings;
use crate::events;
use crate::llm::LlmServer;
use crate::windows;

const OPEN_AT_LOGIN: &str = "general.openAtLogin";
/// Quit must not hang on a `llama-server` that is still starting.
const STOP_TIMEOUT: Duration = Duration::from_secs(3);

/// `quit_app` and the menu bar's Quit: stop the model server and the MCP HTTP server, then exit.
pub async fn quit(app: &AppHandle) {
    if let Some(llm) = app.try_state::<LlmServer>() {
        let _ = tokio::time::timeout(STOP_TIMEOUT, llm.stop()).await;
    }
    if let Some(http) = app.try_state::<McpHttp>() {
        http.stop().await;
    }
    app.exit(0);
}

/// SIGTERM (`kill`, launchd), SIGINT and SIGHUP exit through `quit` instead of ending the
/// process outright, which would orphan `llama-server`.
pub fn quit_on_signals(app: &AppHandle) {
    use tokio::signal::unix::{signal, SignalKind};
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let (Ok(mut term), Ok(mut int), Ok(mut hup)) = (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
            signal(SignalKind::hangup()),
        ) else {
            eprintln!("[system] could not install signal handlers");
            return;
        };
        tokio::select! {
            _ = term.recv() => {}
            _ = int.recv() => {}
            _ = hup.recv() => {}
        }
        quit(&app).await;
    });
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
