//! The local HTTP MCP server: a `copet-mcp --http --port <mcp.httpPort>` child (next to the app
//! executable) that runs while `mcp.paused` is false. It restarts 5 s after it exits, except with
//! exit code 3 (the port is taken), which leaves it stopped in the `portInUse` state. A port
//! change restarts it on the new port.
use std::ffi::OsString;
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
const PORT: &str = "mcp.httpPort";
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

/// The supervisor task and the port it serves.
struct Supervisor {
    task: JoinHandle<()>,
    port: u16,
}

/// Managed state (`app.state::<McpHttp>()`).
pub struct McpHttp {
    state: Arc<Mutex<HttpState>>,
    /// Aborting the supervisor drops the child, which kills it.
    supervisor: tokio::sync::Mutex<Option<Supervisor>>,
}

impl Default for McpHttp {
    fn default() -> Self {
        McpHttp { state: Arc::new(Mutex::new(HttpState::Stopped)), supervisor: tokio::sync::Mutex::new(None) }
    }
}

/// Manages [`McpHttp`], kills a `copet-mcp --http` an earlier Co-pet left behind (it would hold
/// the port), starts the server unless paused and follows `mcp.paused` and `mcp.httpPort`.
pub fn start(app: &AppHandle) {
    app.manage(McpHttp::default());
    match super::mcp_exe() {
        Ok(exe) => reap_orphans(&exe),
        Err(e) => eprintln!("[agent] http: {e}"),
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move { handle.state::<McpHttp>().reconcile(&handle).await });
    let handle = app.clone();
    app.listen_any(events::SETTINGS_CHANGED, move |event| {
        let key = serde_json::from_str::<Value>(event.payload()).ok();
        if !matches!(key.as_ref().and_then(|v| v["key"].as_str()), Some(PAUSED | PORT)) {
            return;
        }
        let handle = handle.clone();
        // Reads the settings itself, so the order these tasks run in doesn't matter.
        tauri::async_runtime::spawn(async move { handle.state::<McpHttp>().reconcile(&handle).await });
    });
}

/// Kills `copet-mcp --http` processes an earlier Co-pet left behind when it died without running
/// its exit path (SIGKILL, crash), and waits until they are gone so the port is free.
fn reap_orphans(exe: &Path) {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let Ok(exe) = exe.canonicalize() else { return };
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Always).with_cmd(UpdateKind::Always),
    );
    for p in sys.processes_by_exact_name("copet-mcp".as_ref()) {
        if is_orphan(p.parent().map(|p| p.as_u32()), p.exe(), p.cmd(), &exe) {
            eprintln!("[agent] http: killing orphaned copet-mcp pid {}", p.pid());
            if let Err(e) = p.kill_and_wait() {
                eprintln!("[agent] http: kill pid {}: {e:?}", p.pid());
            }
        }
    }
}

/// An HTTP server of this app's `copet-mcp` (`exe`, canonical) whose parent is launchd. Stdio
/// servers belong to their agent and exit with its pipe.
fn is_orphan(parent: Option<u32>, exe: Option<&Path>, cmd: &[OsString], ours: &Path) -> bool {
    parent == Some(1) && exe == Some(ours) && cmd.iter().skip(1).any(|a| a == "--http")
}

impl McpHttp {
    pub fn state(&self) -> HttpState {
        *self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Runs the server on `mcp.httpPort` when `mcp.paused` is false (unless it already runs on
    /// that port), stops it otherwise. State changes send `agents-changed`.
    pub(crate) async fn reconcile(&self, app: &AppHandle) {
        let mut sup = self.supervisor.lock().await;
        let (paused, port) = match (settings::read::<bool>(app, PAUSED), settings::read::<u16>(app, PORT)) {
            (Ok(paused), Ok(port)) => (paused, port),
            (Err(e), _) | (_, Err(e)) => {
                eprintln!("[agent] http: settings: {e}");
                return;
            }
        };
        if !paused && keeps(sup.as_ref().map(|s| (s.task.inner().is_finished(), s.port)), port) {
            return;
        }
        if self.stop_locked(&mut sup).await {
            events::agents_changed(app);
        }
        if paused {
            return;
        }
        let exe = match super::mcp_exe() {
            Ok(exe) => exe,
            Err(e) => {
                eprintln!("[agent] http: {e}");
                return;
            }
        };
        let sock = app.state::<Paths>().sock.clone();
        let handle = app.clone();
        let changed = move || events::agents_changed(&handle);
        let task = tauri::async_runtime::spawn(supervise(exe, port, sock, self.state.clone(), RESTART_DELAY, changed));
        *sup = Some(Supervisor { task, port });
    }

    /// Stops the server and waits until the child is killed.
    pub async fn stop(&self) {
        let mut sup = self.supervisor.lock().await;
        self.stop_locked(&mut sup).await;
    }

    /// Returns whether the state changed.
    async fn stop_locked(&self, sup: &mut Option<Supervisor>) -> bool {
        if let Some(s) = sup.take() {
            s.task.abort();
            let _ = s.task.await;
        }
        set(&self.state, HttpState::Stopped)
    }
}

/// Whether the current supervisor (`(finished, port)`) already serves `port`.
fn keeps(current: Option<(bool, u16)>, port: u16) -> bool {
    current.is_some_and(|(finished, p)| !finished && p == port)
}

/// Returns whether the state changed.
fn set(state: &Mutex<HttpState>, s: HttpState) -> bool {
    let mut current = state.lock().unwrap_or_else(|e| e.into_inner());
    std::mem::replace(&mut *current, s) != s
}

/// Runs `exe --http --port <port>` until it exits with [`EXIT_PORT_IN_USE`] or can't be spawned,
/// restarting it `delay` after any other exit. Calls `changed` after each state change.
async fn supervise(
    exe: PathBuf,
    port: u16,
    sock: PathBuf,
    state: Arc<Mutex<HttpState>>,
    delay: Duration,
    changed: impl Fn() + Send + Sync + 'static,
) {
    let update = |s| {
        if set(&state, s) {
            changed();
        }
    };
    loop {
        let mut child = match spawn(&exe, port, &sock) {
            Ok(child) => child,
            Err(e) => {
                eprintln!("[agent] http: {}: {e}", exe.display());
                update(HttpState::Stopped);
                return;
            }
        };
        update(HttpState::Running);
        match child.wait().await {
            Ok(status) if status.code() == Some(EXIT_PORT_IN_USE) => {
                eprintln!("[agent] http: port {port} is in use");
                update(HttpState::PortInUse);
                return;
            }
            Ok(status) => eprintln!("[agent] http: copet-mcp exited ({status}), restarting"),
            Err(e) => eprintln!("[agent] http: wait: {e}, restarting"),
        }
        update(HttpState::Stopped);
        tokio::time::sleep(delay).await;
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
        use std::sync::atomic::{AtomicUsize, Ordering};
        let dir = tempfile::tempdir().unwrap();
        let exe = script(dir.path(), r#"[ "$1 $2 $3" = "--http --port 4823" ] && exit 3; exit 1"#);
        let s = state();
        let changes = Arc::new(AtomicUsize::new(0));
        let counted = changes.clone();
        let changed = move || {
            counted.fetch_add(1, Ordering::SeqCst);
        };
        let sup = supervise(exe, 4823, dir.path().join("mcp.sock"), s.clone(), Duration::ZERO, changed);
        tokio::time::timeout(Duration::from_secs(2), sup).await.expect("no restart after exit code 3");
        assert_eq!(*s.lock().unwrap(), HttpState::PortInUse);
        assert_eq!(changes.load(Ordering::SeqCst), 1, "running → portInUse; running was already set");
    }

    #[tokio::test]
    async fn other_exit_restarts_after_the_delay() {
        const DELAY: Duration = Duration::from_millis(200);
        let dir = tempfile::tempdir().unwrap();
        let runs = dir.path().join("runs");
        let exe = script(dir.path(), &format!("echo run >> '{}'; exit 1", runs.display()));
        let started = std::time::Instant::now();
        let sup = tokio::spawn(supervise(exe, 4823, dir.path().join("mcp.sock"), state(), DELAY, || {}));
        while std::fs::read_to_string(&runs).map_or(0, |r| r.lines().count()) < 2 {
            assert!(started.elapsed() < Duration::from_secs(5), "not restarted");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        sup.abort();
        assert!(started.elapsed() >= DELAY);
    }

    #[tokio::test]
    async fn missing_binary_stops() {
        let dir = tempfile::tempdir().unwrap();
        let s = state();
        let sock = dir.path().join("mcp.sock");
        supervise(dir.path().join("copet-mcp"), 4823, sock, s.clone(), Duration::ZERO, || {}).await;
        assert_eq!(*s.lock().unwrap(), HttpState::Stopped);
    }

    #[test]
    fn a_port_change_restarts() {
        assert!(keeps(Some((false, 4823)), 4823));
        assert!(!keeps(Some((false, 4823)), 4824), "running on the old port");
        assert!(!keeps(Some((true, 4823)), 4823), "stopped (port in use, missing binary)");
        assert!(!keeps(None, 4823));
    }

    #[test]
    fn orphans_are_launchd_children_running_our_http_server() {
        let ours = Path::new("/Applications/Co-pet.app/Contents/MacOS/copet-mcp");
        let cmd = |args: &[&str]| -> Vec<OsString> { args.iter().map(OsString::from).collect() };
        let http = cmd(&[ours.to_str().unwrap(), "--http", "--port", "4823"]);
        assert!(is_orphan(Some(1), Some(ours), &http, ours));
        assert!(!is_orphan(Some(4242), Some(ours), &http, ours), "a running Co-pet's child");
        assert!(!is_orphan(Some(1), Some(ours), &cmd(&[ours.to_str().unwrap(), "--stdio"]), ours), "stdio");
        let other = Path::new("/Applications/Other.app/Contents/MacOS/copet-mcp");
        assert!(!is_orphan(Some(1), Some(other), &http, ours), "another install");
        assert!(!is_orphan(Some(1), None, &http, ours), "exe unknown");
    }
}
