//! Local model runtime: a lazily started `llama-server` child bound to 127.0.0.1 with a random
//! API key, stopped after 300 s without requests and on quit.
pub mod download;
pub mod models;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;
use tauri::{AppHandle, Manager};
use tokio::process::{Child, Command};

use crate::ai::{wire, ChatOpts, Provider};
use crate::db::settings;
use crate::error::{AppError, AppResult};
use crate::paths::Paths;

const IDLE_STOP: Duration = Duration::from_secs(300);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(60);

struct Running {
    child: Child,
    port: u16,
    key: String,
    model: PathBuf,
}

/// Managed state (`app.state::<LlmServer>()`).
pub struct LlmServer {
    running: tokio::sync::Mutex<Option<Running>>,
    last_used: Mutex<Instant>,
    watchdog: Mutex<bool>,
    http: reqwest::Client,
}

impl Default for LlmServer {
    fn default() -> Self {
        LlmServer {
            running: tokio::sync::Mutex::new(None),
            last_used: Mutex::new(Instant::now()),
            watchdog: Mutex::new(false),
            http: reqwest::Client::new(),
        }
    }
}

/// Folder holding `llama-server` and its dylibs: `Contents/Resources/llama/` in the bundle;
/// in debug builds falls back to `src-tauri/resources/llama/` when the resource dir lacks it.
fn server_dir(app: &AppHandle) -> AppResult<PathBuf> {
    let dir = app.path().resource_dir()?.join("llama");
    if !server_exe(&dir).exists() && cfg!(debug_assertions) {
        return Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources").join("llama"));
    }
    Ok(dir)
}

/// `llama-server` (`llama-server.exe` on Windows) inside `dir`.
pub(crate) fn server_exe(dir: &Path) -> PathBuf {
    dir.join(format!("llama-server{}", std::env::consts::EXE_SUFFIX))
}

/// The verified file of the selected model (`ai.modelId`), if it is downloaded.
pub fn active_model_path(app: &AppHandle) -> Option<PathBuf> {
    let id = settings::read::<Option<String>>(app, "ai.modelId").ok().flatten()?;
    let path = models::model_path(&app.state::<Paths>(), models::find(&id)?);
    path.exists().then_some(path)
}

/// Makes `model_id` the active model (`ai.modelId`). A server running another model is stopped
/// so the next request starts the selected one; an already downloaded model re-queues the
/// summaries that waited for one. Error `unknownModel`.
pub async fn select_model(app: &AppHandle, model_id: &str) -> AppResult<()> {
    let spec = models::find(model_id).ok_or(AppError::Code("unknownModel"))?;
    let current = settings::read::<Option<String>>(app, "ai.modelId")?;
    if current.as_deref() != Some(spec.id) {
        settings::set(app, "ai.modelId", &Value::String(spec.id.into()))?;
        app.state::<LlmServer>().stop().await;
    }
    if active_model_path(app).is_some() {
        crate::transcribe::worker::summaries_unblocked(app);
    }
    crate::events::ai_status(app, app.state::<download::Downloader>().status(app));
    Ok(())
}

/// Binds `127.0.0.1:0`, reads the port the OS picked and releases it.
fn free_port() -> AppResult<u16> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

/// Kills `llama-server` processes an earlier Co-pet left behind when it died without running its
/// exit path (SIGKILL, crash): children of launchd running this app's own `llama-server`.
#[cfg(target_os = "macos")]
pub fn reap_orphans(app: &AppHandle) {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let Ok(exe) = server_dir(app).and_then(|d| Ok(server_exe(&d).canonicalize()?)) else {
        return;
    };
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
    );
    for p in sys.processes_by_exact_name("llama-server".as_ref()) {
        if p.parent() == Some(Pid::from_u32(1)) && p.exe() == Some(exe.as_path()) {
            eprintln!("[llm] killing orphaned llama-server pid {}", p.pid());
            p.kill();
        }
    }
}

impl LlmServer {
    /// Starts `llama-server` for the selected model unless it already runs. Error `noModel`
    /// when no verified model is present.
    pub async fn ensure_running(&self, app: &AppHandle) -> AppResult<()> {
        let model = active_model_path(app).ok_or(AppError::Code("noModel"))?;
        self.start_with(&server_dir(app)?, &model).await?;
        let mut started = self.watchdog.lock().unwrap_or_else(|e| e.into_inner());
        if !*started {
            *started = true;
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    let llm = app.state::<LlmServer>();
                    let idle = llm.last_used.lock().unwrap_or_else(|e| e.into_inner()).elapsed();
                    if idle >= IDLE_STOP {
                        llm.stop().await;
                    }
                }
            });
        }
        Ok(())
    }

    /// Spawns `<dir>/llama-server` for `model` unless a live child already serves it, then
    /// waits for `/health` to answer 200.
    pub async fn start_with(&self, dir: &Path, model: &Path) -> AppResult<()> {
        self.touch();
        let mut running = self.running.lock().await;
        if let Some(r) = running.as_mut() {
            if r.model == model && matches!(r.child.try_wait(), Ok(None)) {
                return Ok(());
            }
            let _ = r.child.kill().await;
            *running = None;
        }
        let exe = server_exe(dir);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))?;
        }
        let port = free_port()?;
        let key = uuid::Uuid::new_v4().simple().to_string();
        let mut cmd = Command::new(&exe);
        cmd.arg("-m")
            .arg(model)
            .args(["--host", "127.0.0.1", "--port", &port.to_string(), "--api-key", &key])
            .args(["-c", "8192", "-ngl", "99", "--parallel", "1", "--no-webui"])
            .args(["--reasoning", "off", "--log-disable"])
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        let mut child = crate::platform::process::spawn_child(&mut cmd)?;

        let health = format!("http://127.0.0.1:{port}/health");
        let deadline = Instant::now() + HEALTH_TIMEOUT;
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                return Err(AppError::msg(format!("llama-server exited: {status}")));
            }
            let ok = self
                .http
                .get(&health)
                .timeout(Duration::from_secs(2))
                .send()
                .await
                .is_ok_and(|r| r.status().is_success());
            if ok {
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill().await;
                return Err(AppError::msg("llama-server did not become healthy in 60 s"));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        *running = Some(Running { child, port, key, model: model.to_path_buf() });
        Ok(())
    }

    fn touch(&self) {
        *self.last_used.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
    }

    /// One structured-output chat completion (`opts` name the schema and bound the reply);
    /// returns the parsed JSON content. Only `ai::chat_json` calls this.
    pub(crate) async fn chat_json(&self, opts: &ChatOpts, system: &str, user: &str, schema: &Value) -> AppResult<Value> {
        self.touch();
        let (port, key) = {
            let running = self.running.lock().await;
            let r = running.as_ref().ok_or(AppError::Code("notRunning"))?;
            (r.port, r.key.clone())
        };
        let body = wire::chat_body(None, opts, system, user, schema);
        let resp: Value = self
            .http
            .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
            .bearer_auth(key)
            .json(&body)
            .timeout(opts.timeout)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        self.touch();
        let content = wire::reply_text(Provider::Local, &resp)?;
        Ok(serde_json::from_str(&content)?)
    }

    /// Kills the child, if any (idle timeout, model switch, quit).
    pub async fn stop(&self) {
        if let Some(mut r) = self.running.lock().await.take() {
            let _ = r.child.kill().await;
        }
    }
}
