//! Two-way done/open sync for exported tasks. Every 5 min while the app runs, this Mac's live
//! `exported` rows (≤100 per provider, least recently checked first) are read from Notion/Asana
//! and reconciled with the local status; see `reconcile`.
use std::time::Duration;

use rusqlite::{params, Connection};
use tauri::{AppHandle, Manager};

use super::{asana, notion, ExportError, Provider, RemoteState};
use crate::backend;
use crate::db::{now_ms, tasks, Db};
use crate::error::AppResult;
use crate::events;
use crate::model::{Actor, NotionConfig, NotionStatusMap};

const FIRST_DELAY: Duration = Duration::from_secs(60);
pub const INTERVAL: Duration = Duration::from_secs(300);
const BATCH: i64 = 100;
const REQUEST_GAP: Duration = Duration::from_millis(400);

#[derive(Debug, PartialEq)]
pub enum Action {
    Keep,
    Pull(bool),
    Push(bool),
}

/// Local == remote → Keep. With a base: the side that differs from it changed (Pull when the remote
/// did, else Push). Without one: Pull when the remote's last edit is newer than the local status
/// change, else Push.
pub fn reconcile(local_done: bool, local_changed_at: i64, remote_done: bool, remote_modified_at: i64, synced: Option<bool>) -> Action {
    if local_done == remote_done {
        return Action::Keep;
    }
    let remote_changed = match synced {
        Some(base) => remote_done != base,
        None => remote_modified_at > local_changed_at,
    };
    if remote_changed {
        Action::Pull(remote_done)
    } else {
        Action::Push(local_done)
    }
}

#[derive(Debug)]
struct Row {
    export_id: String,
    task_id: String,
    external_id: String,
    local_done: bool,
    local_changed_at: i64,
    synced: Option<bool>,
}

enum Remote {
    Notion(NotionStatusMap),
    Asana,
}

impl Remote {
    async fn read(&self, token: &str, id: &str) -> Result<RemoteState, ExportError> {
        match self {
            Remote::Notion(map) => notion::page_status(token, id, map).await,
            Remote::Asana => asana::task_status(token, id).await,
        }
    }

    async fn write(&self, token: &str, id: &str, done: bool) -> Result<RemoteState, ExportError> {
        match self {
            Remote::Notion(map) => notion::set_page_status(token, id, map, done).await,
            Remote::Asana => asana::set_task_status(token, id, done).await,
        }
    }
}

/// Notion: `Some` only with a live target whose `mapping.status` is set. Asana: always `Some(Asana)`.
fn remote_for(conn: &Connection, p: Provider) -> AppResult<Option<Remote>> {
    Ok(match p {
        Provider::Asana => Some(Remote::Asana),
        Provider::Notion => match super::target(conn, p)? {
            Some(t) => serde_json::from_value::<NotionConfig>(t.config)?.mapping.status.map(Remote::Notion),
            None => None,
        },
    })
}

fn due_rows(conn: &Connection, device: &str, p: Provider) -> AppResult<Vec<Row>> {
    let mut stmt = conn.prepare(
        "SELECT e.id, e.task_id, e.external_id, t.status = 'done',
           (SELECT COALESCE(MAX(MAX(ev.at, COALESCE(ev.undone_at, 0))), 0) FROM task_events ev
              WHERE ev.task_id = e.task_id AND ev.action IN ('created','done','reopened')),
           s.synced_done
         FROM task_exports e
         JOIN tasks t ON t.id = e.task_id AND t.deleted_at IS NULL
         LEFT JOIN export_status s ON s.export_id = e.id AND s.external_id = e.external_id
         WHERE e.provider = ?1 AND e.device_id = ?2 AND e.state = 'exported' AND e.deleted_at IS NULL
           AND e.external_id IS NOT NULL
         ORDER BY s.checked_at IS NOT NULL, s.checked_at, e.id
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![p.as_str(), device, BATCH], |r| {
        Ok(Row {
            export_id: r.get(0)?,
            task_id: r.get(1)?,
            external_id: r.get(2)?,
            local_done: r.get(3)?,
            local_changed_at: r.get(4)?,
            synced: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Upserts the base: `synced_done = done`, `external_id`, `remote_modified_at`, `checked_at = now`.
fn record(conn: &Connection, export_id: &str, external_id: &str, done: bool, remote_modified_at: Option<i64>, now: i64) -> AppResult<()> {
    conn.execute(
        "INSERT INTO export_status (export_id, external_id, synced_done, remote_modified_at, checked_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(export_id) DO UPDATE SET external_id = excluded.external_id, synced_done = excluded.synced_done,
           remote_modified_at = COALESCE(excluded.remote_modified_at, export_status.remote_modified_at), checked_at = excluded.checked_at",
        params![export_id, external_id, done, remote_modified_at, now],
    )?;
    Ok(())
}

/// Only `checked_at` (inserting `synced_done = local_done` when there is no row yet).
fn touch(conn: &Connection, row: &Row, now: i64) -> AppResult<()> {
    conn.execute(
        "INSERT INTO export_status (export_id, external_id, synced_done, checked_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(export_id) DO UPDATE SET checked_at = excluded.checked_at",
        params![row.export_id, row.external_id, row.local_done, now],
    )?;
    Ok(())
}

/// Remote gone: `task_exports.deleted_at = now, updated_at = now`; deletes the `export_status` row.
fn gone(conn: &Connection, export_id: &str, now: i64) -> AppResult<()> {
    conn.execute("UPDATE task_exports SET deleted_at = ?2, updated_at = ?2 WHERE id = ?1", params![export_id, now])?;
    conn.execute("DELETE FROM export_status WHERE export_id = ?1", [export_id])?;
    Ok(())
}

/// `set_done(.., &Actor::Service(p))`, then `record(done, Some(remote_modified_at))`.
fn apply_pull(conn: &mut Connection, row: &Row, p: Provider, done: bool, remote_modified_at: i64, now: i64) -> AppResult<()> {
    tasks::set_done(conn, &row.task_id, done, &Actor::Service(p))?;
    record(conn, &row.export_id, &row.external_id, done, Some(remote_modified_at), now)
}

/// Called by the export queue when a job is exported: the base is the payload's `done`.
pub fn on_exported(conn: &Connection, export_id: &str, external_id: &str, done: bool, now: i64) -> AppResult<()> {
    record(conn, export_id, external_id, done, None, now)
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_DELAY).await;
        loop {
            cycle(&app).await;
            tokio::time::sleep(INTERVAL).await;
        }
    });
}

async fn cycle(app: &AppHandle) {
    if backend::ensure_network(app).is_err() {
        return;
    }
    for p in Provider::ALL {
        sync_provider(app, p).await;
    }
}

/// Runs `f` on the database connection; the lock is released before the caller awaits again.
fn with_conn<T>(app: &AppHandle, f: impl FnOnce(&mut Connection) -> AppResult<T>) -> AppResult<T> {
    let db = app.state::<Db>();
    let mut conn = db.conn();
    f(&mut conn)
}

/// Logs a failed database write of the cycle; the row is retried on a later cycle.
fn logged(p: Provider, r: AppResult<()>) -> bool {
    if let Err(e) = &r {
        eprintln!("[export-status] {}: {e}", p.as_str());
    }
    r.is_ok()
}

fn reconnect(app: &AppHandle, p: Provider) {
    if let Err(e) = super::mark_needs_reconnect(p) {
        eprintln!("[export-status] {} mark reconnect: {e}", p.as_str());
    }
    events::exports_changed(app, vec![]);
}

async fn sync_provider(app: &AppHandle, p: Provider) {
    match super::keychain_token(p) {
        Ok(Some(t)) if !t.needs_reconnect => {}
        Ok(_) => return,
        Err(e) => {
            eprintln!("[export-status] {} keychain: {e}", p.as_str());
            return;
        }
    }
    let loaded = with_conn(app, |conn| {
        let device = super::device_id(conn)?;
        let Some(remote) = remote_for(conn, p)? else { return Ok(None) };
        let rows = due_rows(conn, &device, p)?;
        Ok((!rows.is_empty()).then_some((remote, rows)))
    });
    let (remote, rows) = match loaded {
        Ok(Some(x)) => x,
        Ok(None) => return,
        Err(e) => {
            eprintln!("[export-status] {}: {e}", p.as_str());
            return;
        }
    };
    let mut access = match super::token(app, p).await.map_err(super::token_error) {
        Ok(a) => a,
        Err(ExportError::Unauthorized) => return reconnect(app, p),
        Err(_) => return,
    };
    let (mut refreshed, mut push_blocked) = (false, false);
    let (mut changed, mut gone_ids) = (Vec::new(), Vec::new());
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(REQUEST_GAP).await;
        }
        let mut read = remote.read(&access, &row.external_id).await;
        if read == Err(ExportError::Unauthorized) && !refreshed {
            refreshed = true;
            read = match super::refreshed_token(app, p).await {
                Ok(fresh) => {
                    access = fresh;
                    remote.read(&access, &row.external_id).await
                }
                Err(ExportError::Invalid(msg)) => {
                    eprintln!("[export-status] {} refresh: {msg}", p.as_str());
                    Err(ExportError::Retryable)
                }
                Err(e) => Err(e),
            };
        }
        let now = now_ms();
        let (done, modified_at) = match read {
            Ok(RemoteState::Status { done, modified_at }) => (done, modified_at),
            Ok(RemoteState::Gone) | Err(ExportError::TargetMissing) => {
                if logged(p, with_conn(app, |c| gone(c, &row.export_id, now))) {
                    gone_ids.push(row.task_id.clone());
                }
                continue;
            }
            Ok(RemoteState::Unknown) | Err(ExportError::Invalid(_)) => {
                if let Err(ExportError::Invalid(msg)) = &read {
                    eprintln!("[export-status] {}: {msg}", row.export_id);
                }
                logged(p, with_conn(app, |c| touch(c, row, now)));
                continue;
            }
            Err(ExportError::Unauthorized) => {
                reconnect(app, p);
                break;
            }
            Err(ExportError::RateLimited(_) | ExportError::Retryable) => break,
        };
        match reconcile(row.local_done, row.local_changed_at, done, modified_at, row.synced) {
            Action::Keep => {
                logged(p, with_conn(app, |c| record(c, &row.export_id, &row.external_id, done, Some(modified_at), now)));
            }
            Action::Pull(d) => {
                if logged(p, with_conn(app, |c| apply_pull(c, row, p, d, modified_at, now))) {
                    changed.push(row.task_id.clone());
                }
            }
            Action::Push(_) if push_blocked => {
                logged(p, with_conn(app, |c| touch(c, row, now)));
            }
            Action::Push(d) => {
                tokio::time::sleep(REQUEST_GAP).await;
                let written = remote.write(&access, &row.external_id, d).await;
                let now = now_ms();
                match written {
                    Ok(RemoteState::Status { modified_at: m, .. }) => {
                        logged(p, with_conn(app, |c| record(c, &row.export_id, &row.external_id, d, Some(m), now)));
                    }
                    Ok(RemoteState::Unknown) => {
                        logged(p, with_conn(app, |c| record(c, &row.export_id, &row.external_id, d, None, now)));
                    }
                    Ok(RemoteState::Gone) | Err(ExportError::TargetMissing) => {
                        if logged(p, with_conn(app, |c| gone(c, &row.export_id, now))) {
                            gone_ids.push(row.task_id.clone());
                        }
                    }
                    Err(ExportError::Unauthorized) => {
                        push_blocked = true;
                        eprintln!("[export-status] {} refused a status write", p.as_str());
                        logged(p, with_conn(app, |c| touch(c, row, now)));
                    }
                    Err(ExportError::RateLimited(_) | ExportError::Retryable) => break,
                    Err(ExportError::Invalid(msg)) => {
                        eprintln!("[export-status] {}: {msg}", row.export_id);
                        logged(p, with_conn(app, |c| touch(c, row, now)));
                    }
                }
            }
        }
    }
    if !changed.is_empty() {
        tasks::notify_changed(app, changed);
    }
    if !gone_ids.is_empty() {
        events::exports_changed(app, gone_ids);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{enqueue, tests::task};

    #[test]
    fn reconcile_rules() {
        assert_eq!(reconcile(true, 0, true, 0, None), Action::Keep);
        assert_eq!(reconcile(true, 5, true, 9, Some(false)), Action::Keep);
        assert_eq!(reconcile(false, 0, true, 0, Some(false)), Action::Pull(true));
        assert_eq!(reconcile(true, 0, false, 0, Some(false)), Action::Push(true));
        assert_eq!(reconcile(false, 0, true, 0, Some(true)), Action::Push(false));
        assert_eq!(reconcile(false, 1000, true, 2000, None), Action::Pull(true));
        assert_eq!(reconcile(true, 1000, false, 2000, None), Action::Pull(false));
        assert_eq!(reconcile(false, 1000, true, 1000, None), Action::Push(false));
        assert_eq!(reconcile(true, 1000, false, 500, None), Action::Push(true));
    }

    /// A live `exported` Asana row of a new task, with `external_id`.
    fn exported(conn: &Connection, title: &str, external_id: &str) -> (String, String) {
        let t = task(conn, title);
        enqueue(conn, &t, Provider::Asana, None, 1).unwrap();
        let id = format!("{t}:asana");
        conn.execute("UPDATE task_exports SET state = 'exported', external_id = ?2 WHERE id = ?1", params![id, external_id])
            .unwrap();
        (t, id)
    }

    fn device(conn: &Connection) -> String {
        crate::export::device_id(conn).unwrap()
    }

    #[test]
    fn due_rows_filter_and_order() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let conn = db.conn();
        let (_, checked_late) = exported(&conn, "A", "a");
        let (_, checked_early) = exported(&conn, "B", "b");
        let (_, fresh) = exported(&conn, "C", "c");
        let (_, rebased) = exported(&conn, "D", "d");
        let (_, other) = exported(&conn, "E", "e");
        conn.execute("UPDATE task_exports SET device_id = 'other-mac' WHERE id = ?1", [&other]).unwrap();
        let (_, queued) = exported(&conn, "F", "f");
        conn.execute("UPDATE task_exports SET state = 'queued' WHERE id = ?1", [&queued]).unwrap();
        let (trashed_task, _) = exported(&conn, "G", "g");
        conn.execute("UPDATE tasks SET deleted_at = 5 WHERE id = ?1", [&trashed_task]).unwrap();
        let (_, no_external) = exported(&conn, "H", "h");
        conn.execute("UPDATE task_exports SET external_id = NULL WHERE id = ?1", [&no_external]).unwrap();
        let (_, deleted) = exported(&conn, "I", "i");
        conn.execute("UPDATE task_exports SET deleted_at = 5 WHERE id = ?1", [&deleted]).unwrap();
        record(&conn, &checked_late, "a", true, None, 30).unwrap();
        record(&conn, &checked_early, "b", false, None, 10).unwrap();
        record(&conn, &rebased, "old", true, None, 20).unwrap();

        let rows = due_rows(&conn, &device(&conn), Provider::Asana).unwrap();
        let order: Vec<(&str, Option<bool>)> = rows.iter().map(|r| (r.export_id.as_str(), r.synced)).collect();
        // Unchecked first (a base for an older external id does not count), by id; then by `checked_at`.
        let mut unchecked = vec![(fresh.as_str(), None), (rebased.as_str(), None)];
        unchecked.sort();
        unchecked.extend([(checked_early.as_str(), Some(false)), (checked_late.as_str(), Some(true))]);
        assert_eq!(order, unchecked);
        assert!(due_rows(&conn, &device(&conn), Provider::Notion).unwrap().is_empty());
    }

    #[test]
    fn local_changed_at_counts_undo() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let conn = db.conn();
        let (t, _) = exported(&conn, "A", "a");
        conn.execute(
            "INSERT INTO task_events (id, task_id, at, actor, action, undone_at, updated_at) VALUES
               ('e1', ?1, 10, 'user', 'done', 50, 50), ('e2', ?1, 90, 'user', 'edited', NULL, 90)",
            [&t],
        )
        .unwrap();
        let rows = due_rows(&conn, &device(&conn), Provider::Asana).unwrap();
        assert_eq!(rows[0].local_changed_at, 50);
    }

    #[test]
    fn pull_records_service_actor() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let mut conn = db.conn();
        let (t, _) = exported(&conn, "A", "9");
        let row = due_rows(&conn, &device(&conn), Provider::Asana).unwrap().remove(0);
        apply_pull(&mut conn, &row, Provider::Asana, true, 7, 8).unwrap();
        let detail = tasks::get(&conn, &t).unwrap();
        assert_eq!(detail.task.status, "done");
        let ev = &detail.events[0];
        assert_eq!((ev.actor.as_str(), ev.action.as_str()), ("asana", "done"));
        let base: (bool, Option<i64>) = conn
            .query_row("SELECT synced_done, remote_modified_at FROM export_status", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(base, (true, Some(7)));
        tasks::undo_event(&mut conn, &ev.id).unwrap();
        assert_eq!(tasks::summary(&conn, &t).unwrap().status, "open");
    }

    #[test]
    fn gone_frees_task_for_reexport() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let conn = db.conn();
        let (t, id) = exported(&conn, "A", "9");
        record(&conn, &id, "9", false, None, 2).unwrap();
        gone(&conn, &id, 3).unwrap();
        assert!(tasks::summary(&conn, &t).unwrap().export.is_none());
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM export_status", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        assert_eq!(enqueue(&conn, &t, Provider::Asana, None, 4).unwrap().state, "queued");
    }

    #[test]
    fn on_exported_sets_base() {
        let db = crate::db::Db::open_in_memory().unwrap();
        let conn = db.conn();
        on_exported(&conn, "t:asana", "p1", true, 5).unwrap();
        on_exported(&conn, "t:asana", "p2", false, 6).unwrap();
        let rows: Vec<(String, bool, i64)> = conn
            .prepare("SELECT external_id, synced_done, checked_at FROM export_status")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(rows, vec![("p2".to_string(), false, 6)]);
    }
}
