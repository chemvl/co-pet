//! The local MCP server's Rust side. The `copet-mcp` sidecar forwards each tool call over a Unix
//! socket on macOS or a named pipe on Windows ([`socket`]); [`tools::dispatch`] identifies the
//! client ([`clients`]), applies pause, write approval and agent visibility, does the work and
//! logs it ([`activity`]). Agents never touch SQLite.
pub mod activity;
pub mod clients;
pub mod http;
/// Only Windows serves the pipe; macOS compiles the name for its test.
#[cfg(any(windows, test))]
pub(crate) mod pipe;
mod socket;
pub mod tools;

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::capture::{self, CaptureInput, CaptureSource};
use crate::db::{items, now_ms, settings, tasks, Db};
use crate::error::{AppError, AppResult};
use crate::events::{self, PetReaction};
use crate::export::queue::ExportQueue;
use crate::i18n::{self, Lang};
use crate::model::{ActivityRow, CaptureResult, HttpClientCreated, McpClientView, McpStatus, McpToken, NextCard};
use crate::paths::Paths;

/// At most one `glance` reaction per this gap.
const GLANCE_GAP: Duration = Duration::from_secs(5);

/// Deletes activity older than 90 days, starts the socket and the local HTTP server
/// ([`http`]). Needs `Paths`, `Db` and the export queue managed.
pub fn start(app: &AppHandle) {
    match activity::prune(&app.state::<Db>().conn(), now_ms()) {
        Ok(0) => {}
        Ok(n) => eprintln!("[agent] pruned {n} activity row(s)"),
        Err(e) => eprintln!("[agent] prune failed: {e}"),
    }
    socket::start(app);
    http::start(app);
}

/// The bundled `copet-mcp` (`copet-mcp.exe` on Windows), next to the app executable.
pub fn mcp_exe() -> AppResult<PathBuf> {
    Ok(std::env::current_exe()?.with_file_name(format!("copet-mcp{}", std::env::consts::EXE_SUFFIX)))
}

pub fn status(app: &AppHandle) -> AppResult<McpStatus> {
    let db = app.state::<Db>();
    let (paused, port) = {
        let conn = db.conn();
        (settings::get::<bool>(&conn, "mcp.paused")?, settings::get::<u16>(&conn, "mcp.httpPort")?)
    };
    Ok(McpStatus {
        paused,
        http_state: app.state::<http::McpHttp>().state(),
        http_url: format!("http://127.0.0.1:{port}/mcp"),
        stdio_command: format!("{} --stdio", mcp_exe()?.display()),
    })
}

/// Pausing makes every call answer `paused` and stops the HTTP server (which follows the setting).
pub fn set_paused(app: &AppHandle, paused: bool) -> AppResult<()> {
    settings::set(app, "mcp.paused", &Value::Bool(paused))?;
    events::agents_changed(app);
    Ok(())
}

fn setup(app: &AppHandle) -> AppResult<clients::Setup<'static>> {
    static CLAUDE: clients::LoginShell = clients::LoginShell;
    Ok(clients::Setup { home: app.path().home_dir()?, command: mcp_exe()?, claude: &CLAUDE })
}

/// Runs `f` on the blocking pool (config files, the login shell, SQLite).
async fn blocking<T: Send + 'static>(
    app: &AppHandle,
    f: impl FnOnce(&AppHandle) -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || f(&app)).await.map_err(|e| AppError::msg(e.to_string()))?
}

pub async fn list_clients(app: &AppHandle) -> AppResult<Vec<McpClientView>> {
    blocking(app, |app| {
        let installed = clients::installed(&app.path().home_dir()?);
        clients::list(&app.state::<Db>().conn(), &installed)
    })
    .await
}

/// "Add to …" for a known client key. `invalid`, `configUnreadable`.
pub async fn add_client(app: &AppHandle, key: String) -> AppResult<McpClientView> {
    let view = blocking(app, move |app| clients::add(&app.state::<Db>(), &setup(app)?, &key, now_ms())).await?;
    events::agents_changed(app);
    Ok(view)
}

/// `notFound`.
pub fn approve_client(app: &AppHandle, id: &str) -> AppResult<McpToken> {
    let token = clients::approve(&app.state::<Db>().conn(), id, now_ms())?;
    events::agents_changed(app);
    Ok(McpToken { token })
}

/// `notFound`.
pub fn revoke_client(app: &AppHandle, id: &str) -> AppResult<()> {
    clients::revoke(&app.state::<Db>().conn(), id, now_ms())?;
    events::agents_changed(app);
    Ok(())
}

/// `notFound`, `configUnreadable` (the client stays).
pub async fn remove_client(app: &AppHandle, id: String) -> AppResult<()> {
    blocking(app, move |app| clients::remove(&app.state::<Db>(), &setup(app)?, &id, now_ms())).await?;
    events::agents_changed(app);
    Ok(())
}

/// `invalid` for an empty or oversized name.
pub fn create_http_client(app: &AppHandle, name: &str) -> AppResult<HttpClientCreated> {
    let (client, token) = clients::create_http(&app.state::<Db>().conn(), name, now_ms())?;
    events::agents_changed(app);
    Ok(HttpClientCreated { client, token })
}

pub fn list_activity(app: &AppHandle, changes_only: bool, limit: i64) -> AppResult<Vec<ActivityRow>> {
    activity::list(&app.state::<Db>().conn(), changes_only, limit)
}

/// `notFound`, `alreadyUndone`, `notUndoable`.
pub fn undo_activity(app: &AppHandle, id: &str) -> AppResult<()> {
    let undone = activity::undo_activity(&mut app.state::<Db>().conn(), id)?;
    if !undone.item_ids.is_empty() {
        items::notify_changed(app, undone.item_ids);
    }
    if !undone.task_ids.is_empty() {
        tasks::notify_changed(app, undone.task_ids);
    }
    events::agents_changed(app);
    Ok(())
}

/// The running app as the tools' [`tools::Host`].
pub(crate) struct AppHost(pub AppHandle);

impl tools::Host for AppHost {
    fn db(&self) -> &Db {
        self.0.state::<Db>().inner()
    }

    fn paths(&self) -> &Paths {
        self.0.state::<Paths>().inner()
    }

    fn lang(&self) -> Lang {
        i18n::lang(&self.0)
    }

    fn capture(&self, input: CaptureInput, source: CaptureSource) -> CaptureResult {
        capture::capture(&self.0, input, source)
    }

    /// Runs on the blocking pool, so it may block on the async pick.
    fn whats_next(&self) -> Result<Option<NextCard>, AppError> {
        tauri::async_runtime::block_on(crate::next::pick(&self.0, "agent"))
    }

    fn items_changed(&self, ids: Vec<String>) {
        items::notify_changed(&self.0, ids);
    }

    fn tasks_changed(&self, ids: Vec<String>) {
        tasks::notify_changed(&self.0, ids);
    }

    fn task_done(&self) {
        events::celebrate(&self.0);
        crate::next::after_done(&self.0);
    }

    fn export_queued(&self, task_id: String) {
        self.0.state::<ExportQueue>().poke();
        events::exports_changed(&self.0, vec![task_id]);
    }

    /// `pet-reaction glance`, at most once per 5 s; the pet drops it while asleep.
    fn glance(&self) {
        static LAST: Mutex<Option<Instant>> = Mutex::new(None);
        let now = Instant::now();
        {
            let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
            if last.is_some_and(|t| now.duration_since(t) < GLANCE_GAP) {
                return;
            }
            *last = Some(now);
        }
        events::pet_reaction(&self.0, PetReaction::new("glance"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_exe_sits_next_to_the_app_with_the_platform_suffix() {
        let exe = mcp_exe().unwrap();
        assert_eq!(exe.parent(), std::env::current_exe().unwrap().parent());
        assert_eq!(exe.file_name().unwrap(), format!("copet-mcp{}", std::env::consts::EXE_SUFFIX).as_str());
    }
}
