//! The local MCP server's Rust side. The `copet-mcp` sidecar forwards each tool call over a Unix
//! socket ([`socket`]); [`tools::dispatch`] identifies the client ([`clients`]), applies pause,
//! write approval and agent visibility, does the work and logs it ([`activity`]). Agents never
//! touch SQLite.
pub mod activity;
pub mod clients;
pub mod http;
mod socket;
pub mod tools;

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use crate::capture::{self, CaptureInput, CaptureSource};
use crate::db::{items, now_ms, tasks, Db};
use crate::error::AppError;
use crate::events::{self, PetReaction};
use crate::export::queue::ExportQueue;
use crate::i18n::{self, Lang};
use crate::model::{CaptureResult, NextCard};
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
