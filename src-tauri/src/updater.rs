//! Updates from the signed `latest.json` feed (Step 14.3). Auto-check 30 s after launch and every
//! 6 h while `updates.autoCheck` is on and `privacy.localOnly` is off. A found update downloads
//! in the background and waits as `ready`; only `install` (a user action) applies it.
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::Connection;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
use url::Url;

use crate::agent::http::McpHttp;
use crate::db::{settings, Db};
use crate::error::{AppError, AppResult};
use crate::model::{UpdateState, UpdateStatus};
use crate::transcribe::recorder::Recorder;
use crate::{backend, events, system, tray_icon};

pub const FIRST_CHECK: Duration = Duration::from_secs(30);
pub const EVERY: Duration = Duration::from_secs(6 * 3600);
/// Build-time override of the feed (`scripts/release.ts --prefix copet-test`).
const FEED: Option<&str> = option_env!("COPET_UPDATE_FEED");

#[derive(Default)]
pub struct Updates {
    /// `None` = idle.
    status: Mutex<Option<UpdateStatus>>,
    /// The downloaded, verified update waiting for `install`.
    ready: tokio::sync::Mutex<Option<(Update, Vec<u8>)>>,
    /// One check at a time.
    running: tokio::sync::Mutex<()>,
}

impl Updates {
    pub fn status(&self) -> UpdateStatus {
        self.lock().clone().unwrap_or_else(|| status(UpdateState::Idle))
    }

    /// Stores `s`, emits `update-status` and rebuilds the menu bar menu.
    fn set(&self, app: &AppHandle, s: UpdateStatus) {
        *self.lock() = Some(s.clone());
        events::update_status(app, s);
        if let Err(e) = tray_icon::refresh(app) {
            eprintln!("[tray] menu rebuild: {e}");
        }
    }

    fn lock(&self) -> MutexGuard<'_, Option<UpdateStatus>> {
        self.status.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn status(state: UpdateState) -> UpdateStatus {
    UpdateStatus { state, version: None, notes: None, progress: None, error: None }
}

fn failed() -> UpdateStatus {
    UpdateStatus { error: Some("updateFailed".into()), ..status(UpdateState::Failed) }
}

/// Whether the background check may run: `updates.autoCheck` on and `privacy.localOnly` off.
pub fn auto_allowed(conn: &Connection) -> AppResult<bool> {
    Ok(settings::get::<bool>(conn, "updates.autoCheck")? && !settings::get::<bool>(conn, "privacy.localOnly")?)
}

/// Spawns the auto-check loop: first after `FIRST_CHECK`, then every `EVERY`.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            let allowed = auto_allowed(&app.state::<Db>().conn());
            match allowed {
                Ok(true) => {
                    let _ = check(&app).await;
                }
                Ok(false) => {}
                Err(e) => eprintln!("[updater] settings: {e}"),
            }
            tokio::time::sleep(EVERY).await;
        }
    });
}

/// Checks the feed and downloads a found update. A `ready` or downloading update is returned as
/// is; a plugin error ends as `failed` with `updateFailed`. `localOnly` while offline mode is on.
pub async fn check(app: &AppHandle) -> AppResult<UpdateStatus> {
    backend::ensure_network(app)?;
    let updates = app.state::<Updates>();
    if busy(&updates.status()) {
        return Ok(updates.status());
    }
    let _running = updates.running.lock().await;
    // A check that held the lock meanwhile may have found one.
    let current = updates.status();
    if busy(&current) {
        return Ok(current);
    }
    updates.set(app, status(UpdateState::Checking));
    let done = match find(app, &updates).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[updater] {e}");
            failed()
        }
    };
    updates.set(app, done.clone());
    Ok(done)
}

fn busy(s: &UpdateStatus) -> bool {
    matches!(s.state, UpdateState::Ready | UpdateState::Downloading)
}

/// Checks and downloads; `upToDate` or `ready`. Sends `downloading` progress steps of ≥ 1 %.
async fn find(app: &AppHandle, updates: &Updates) -> tauri_plugin_updater::Result<UpdateStatus> {
    let updater = match FEED {
        Some(f) => app.updater_builder().endpoints(vec![Url::parse(f)?])?.build()?,
        None => app.updater()?,
    };
    let Some(update) = updater.check().await? else {
        return Ok(status(UpdateState::UpToDate));
    };
    let downloading = |progress: f64| UpdateStatus {
        version: Some(update.version.clone()),
        progress: Some(progress),
        ..status(UpdateState::Downloading)
    };
    updates.set(app, downloading(0.0));
    let (mut received, mut sent) = (0u64, 0.0);
    let bytes = update
        .download(
            |chunk, total| {
                received += chunk as u64;
                let Some(total) = total.filter(|t| *t > 0) else { return };
                let progress = (received as f64 / total as f64).min(1.0);
                if progress - sent >= 0.01 {
                    sent = progress;
                    updates.set(app, downloading(progress));
                }
            },
            || {},
        )
        .await?;
    let ready = UpdateStatus {
        version: Some(update.version.clone()),
        notes: update.body.clone(),
        ..status(UpdateState::Ready)
    };
    *updates.ready.lock().await = Some((update, bytes));
    Ok(ready)
}

/// Stops the recorder, model and MCP servers, installs the ready update and restarts. `recording`
/// while recording, `noUpdate` without a ready update, `updateFailed` when installing fails (the
/// MCP server is started again; `llama-server` restarts on its next use).
pub async fn install(app: &AppHandle) -> AppResult<()> {
    if app.try_state::<Recorder>().is_some_and(|r| r.is_active()) {
        return Err(AppError::Code("recording"));
    }
    let updates = app.state::<Updates>();
    let Some((update, bytes)) = updates.ready.lock().await.take() else {
        return Err(AppError::Code("noUpdate"));
    };
    system::shutdown(app).await;
    if let Err(e) = update.install(&bytes) {
        eprintln!("[updater] {e}");
        updates.set(app, failed());
        if let Some(http) = app.try_state::<McpHttp>() {
            http.reconcile(app).await;
        }
        return Err(AppError::Code("updateFailed"));
    }
    // Not `restart`: from a worker thread it blocks forever, so this command would never return.
    app.request_restart();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn auto_check_follows_settings() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        assert!(auto_allowed(&conn).unwrap());
        settings::put(&conn, "privacy.localOnly", &json!(true)).unwrap();
        assert!(!auto_allowed(&conn).unwrap());
        settings::put(&conn, "privacy.localOnly", &json!(false)).unwrap();
        settings::put(&conn, "updates.autoCheck", &json!(false)).unwrap();
        assert!(!auto_allowed(&conn).unwrap());
    }

    #[test]
    fn status_defaults_idle() {
        assert_eq!(Updates::default().status().state, UpdateState::Idle);
    }

    #[test]
    fn update_status_serializes_camel_case() {
        assert_eq!(
            serde_json::to_value(status(UpdateState::UpToDate)).unwrap(),
            json!({"state": "upToDate", "version": null, "notes": null, "progress": null, "error": null})
        );
    }
}
