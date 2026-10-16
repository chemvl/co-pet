//! The local HTTP MCP server: a `copet-mcp --http --port <mcp.httpPort>` child (next to the app
//! executable) that runs while `mcp.paused` is false. It restarts 5 s after it exits, except with
//! exit code 3 (the port is taken), which leaves it stopped in the `portInUse` state.
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Listener, Manager};
use tauri::async_runtime::JoinHandle;
use tokio::process::Command;

use crate::db::settings;
use crate::events;
use crate::paths::Paths;

const PAUSED: &str = "mcp.paused";
const RESTART_DELAY: Duration = Duration::from_secs(5);
/// `copet-mcp`'s exit code when the port is in use.
const EXIT_PORT_IN_USE: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HttpState {
    Running,
    Stopped,
    PortInUse,
}

/// Managed state (`app.state::<McpHttp>()`).
pub struct McpHttp {
    state: Arc<Mutex<HttpState>>,
    /// The supervisor; aborting it drops the child, which kills it.
    task: tokio::sync::Mutex<Option<JoinHandle<()>>>,
}

impl Default for McpHttp {
    fn default() -> Self {
        McpHttp { state: Arc::new(Mutex::new(HttpState::Stopped)), task: tokio::sync::Mutex::new(None) }
    }
}

/// Manages [`McpHttp`], starts the server unless paused and follows `mcp.paused`.
pub fn start(app: &AppHandle) {
    app.manage(McpHttp::default());
    let handle = app.clone();
    tauri::async_runtime::spawn(async move { handle.state::<McpHttp>().reconcile(&handle).await });
    let handle = app.clone();
    app.listen_any(events::SETTINGS_CHANGED, move |event| {
        let key = serde_json::from_str::<Value>(event.payload()).ok();
        if key.as_ref().and_then(|v| v["key"].as_str()) != Some(PAUSED) {
            return;
        }
        let handle = handle.clone();
        // Reads the setting itself, so the order these tasks run in doesn't matter.
        tauri::async_runtime::spawn(async move { handle.state::<McpHttp>().reconcile(&handle).await });
    });
}

impl McpHttp {
    /// Runs the server when `mcp.paused` is false (unless it already runs), stops it otherwise.
    async fn reconcile(&self, app: &AppHandle) {
        let mut task = self.task.lock().await;
        let (paused, port) = match (settings::read::<bool>(app, PAUSED), settings::read::<u16>(app, "mcp.httpPort")) {
            (Ok(paused), Ok(port)) => (paused, port),
            (Err(e), _) | (_, Err(e)) => {
                eprintln!("[agent] http: settings: {e}");
                return;
            }
        };
        if paused {
            self.stop_locked(&mut task).await;
            return;
        }
        if task.as_ref().is_some_and(|t| !t.inner().is_finished()) {
            return;
        }
        let exe = match std::env::current_exe() {
            Ok(exe) => exe.with_file_name("copet-mcp"),
            Err(e) => {
                eprintln!("[agent] http: current_exe: {e}");
                return;
            }
        };
        let sock = app.state::<Paths>().sock.clone();
        *task = Some(tauri::async_runtime::spawn(supervise(exe, port, sock, self.state.clone())));
    }

    /// Stops the server and waits until the child is killed.
    pub async fn stop(&self) {
        let mut task = self.task.lock().await;
        self.stop_locked(&mut task).await;
    }

    async fn stop_locked(&self, task: &mut Option<JoinHandle<()>>) {
        if let Some(t) = task.take() {
            t.abort();
            let _ = t.await;
        }
        set(&self.state, HttpState::Stopped);
    }
}

fn set(state: &Mutex<HttpState>, s: HttpState) {
    *state.lock().unwrap_or_else(|e| e.into_inner()) = s;
}

/// Runs `exe --http --port <port>` until it exits with [`EXIT_PORT_IN_USE`] or can't be spawned,
/// restarting it [`RESTART_DELAY`] after any other exit.
async fn supervise(exe: PathBuf, port: u16, sock: PathBuf, state: Arc<Mutex<HttpState>>) {
    loop {
        let mut child = match spawn(&exe, port, &sock) {
            Ok(child) => child,
            Err(e) => {
                eprintln!("[agent] http: {}: {e}", exe.display());
                set(&state, HttpState::Stopped);
                return;
            }
        };
        set(&state, HttpState::Running);
        match child.wait().await {
            Ok(status) if status.code() == Some(EXIT_PORT_IN_USE) => {
                eprintln!("[agent] http: port {port} is in use");
                set(&state, HttpState::PortInUse);
                return;
            }
            Ok(status) => eprintln!("[agent] http: copet-mcp exited ({status}), restarting"),
            Err(e) => eprintln!("[agent] http: wait: {e}, restarting"),
        }
        set(&state, HttpState::Stopped);
        tokio::time::sleep(RESTART_DELAY).await;
    }
}

fn spawn(exe: &Path, port: u16, sock: &Path) -> std::io::Result<tokio::process::Child> {
    Command::new(exe)
        .args(["--http", "--port", &port.to_string()])
        .env("COPET_SOCKET", sock)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .kill_on_drop(true)
        .spawn()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn script(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("copet-mcp");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn state() -> Arc<Mutex<HttpState>> {
        Arc::new(Mutex::new(HttpState::Running))
    }

    #[tokio::test]
    async fn port_in_use_stops_without_restart() {
        let dir = tempfile::tempdir().unwrap();
        let exe = script(dir.path(), r#"[ "$1 $2 $3" = "--http --port 4823" ] && exit 3; exit 1"#);
        let s = state();
        tokio::time::timeout(Duration::from_secs(2), supervise(exe, 4823, dir.path().join("mcp.sock"), s.clone()))
            .await
            .expect("no restart after exit code 3");
        assert_eq!(*s.lock().unwrap(), HttpState::PortInUse);
    }

    #[tokio::test]
    async fn other_exit_restarts_after_the_delay() {
        let dir = tempfile::tempdir().unwrap();
        let runs = dir.path().join("runs");
        let exe = script(dir.path(), &format!("echo run >> '{}'; exit 1", runs.display()));
        let started = std::time::Instant::now();
        let sup = tokio::spawn(supervise(exe, 4823, dir.path().join("mcp.sock"), state()));
        while std::fs::read_to_string(&runs).map_or(0, |r| r.lines().count()) < 2 {
            assert!(started.elapsed() < Duration::from_secs(10), "not restarted");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        sup.abort();
        assert!(started.elapsed() >= RESTART_DELAY);
    }

    #[tokio::test]
    async fn missing_binary_stops() {
        let dir = tempfile::tempdir().unwrap();
        let s = state();
        supervise(dir.path().join("copet-mcp"), 4823, dir.path().join("mcp.sock"), s.clone()).await;
        assert_eq!(*s.lock().unwrap(), HttpState::Stopped);
    }
}
