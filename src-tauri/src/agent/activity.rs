//! The agent activity log: one `agent_activity` row per tool call, errors included, kept for
//! 90 days. Writes are undoable as a whole.
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::db::{items, new_id, now_ms, tasks};
use crate::error::{AppError, AppResult};
use crate::model::Actor;

const KEEP_MS: i64 = 90 * 86_400_000;
/// `tool` is whatever the client sent; unknown names are cut to this length.
const TOOL_MAX: usize = 64;

/// One call, as logged.
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: String,
    pub client_id: String,
    pub tool: String,
    pub is_write: bool,
    /// What the UI shows per tool: reads `{query?, count}` or `{id, title}`; writes `{title, …}`.
    pub detail: Value,
    pub item_event_ids: Vec<String>,
    pub task_event_ids: Vec<String>,
    pub added_item_ids: Vec<String>,
    /// The error code returned to the agent.
    pub error: Option<&'static str>,
}

impl Entry {
    pub fn new(client_id: &str, tool: &str, is_write: bool) -> Self {
        Entry {
            id: new_id(),
            client_id: client_id.to_owned(),
            tool: tool.chars().take(TOOL_MAX).collect(),
            is_write,
            detail: json!({}),
            item_event_ids: Vec::new(),
            task_event_ids: Vec::new(),
            added_item_ids: Vec::new(),
            error: None,
        }
    }
}

pub fn insert(conn: &Connection, e: &Entry, at: i64) -> AppResult<()> {
    conn.execute(
        "INSERT INTO agent_activity (id, client_id, at, tool, is_write, detail, item_event_ids,
           task_event_ids, added_item_ids, error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            e.id,
            e.client_id,
            at,
            e.tool,
            e.is_write,
            e.detail.to_string(),
            serde_json::to_string(&e.item_event_ids)?,
            serde_json::to_string(&e.task_event_ids)?,
            serde_json::to_string(&e.added_item_ids)?,
            e.error
        ],
    )?;
    Ok(())
}

/// Deletes rows older than 90 days; returns how many.
pub fn prune(conn: &Connection, now: i64) -> AppResult<usize> {
    Ok(conn.execute("DELETE FROM agent_activity WHERE at < ?1", [now - KEEP_MS])?)
}

fn ids(s: &str) -> AppResult<Vec<String>> {
    Ok(serde_json::from_str(s)?)
}

/// Undoes a write call: its item and task events (newest first; ones already undone are
/// skipped), then trashes the items it added. Returns the changed item and task ids. `notFound`,
/// `alreadyUndone`, `notUndoable` (reads, failed calls, `export_task`).
#[allow(dead_code)] // Task 5.3: the `undo_agent_activity` command.
pub fn undo_activity(conn: &mut Connection, id: &str) -> AppResult<Vec<String>> {
    let row: Option<(String, bool, Option<String>, Option<i64>, String, String, String)> = conn
        .query_row(
            "SELECT tool, is_write, error, undone_at, item_event_ids, task_event_ids, added_item_ids
             FROM agent_activity WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        )
        .optional()?;
    let (tool, is_write, error, undone_at, item_events, task_events, added) =
        row.ok_or(AppError::Code("notFound"))?;
    if undone_at.is_some() {
        return Err(AppError::Code("alreadyUndone"));
    }
    if !is_write || error.is_some() || tool == "export_task" {
        return Err(AppError::Code("notUndoable"));
    }
    let mut changed: Vec<String> = Vec::new();
    let mut push = |id: String| {
        if !changed.contains(&id) {
            changed.push(id);
        }
    };
    for ev in ids(&item_events)?.iter().rev() {
        match items::undo_event(conn, ev) {
            Ok(item) => push(item),
            Err(AppError::Code("alreadyUndone")) => {}
            Err(e) => return Err(e),
        }
    }
    for ev in ids(&task_events)?.iter().rev() {
        match tasks::undo_event(conn, ev) {
            Ok(task) => push(task),
            Err(AppError::Code("alreadyUndone")) => {}
            Err(e) => return Err(e),
        }
    }
    let mut live = Vec::new();
    for item in ids(&added)? {
        let row = items::get_row(conn, &item)?;
        if row.deleted_at.is_none() {
            live.push(item);
        }
    }
    items::trash(conn, &live, &Actor::User)?;
    live.into_iter().for_each(&mut push);
    conn.execute("UPDATE agent_activity SET undone_at = ?2 WHERE id = ?1", params![id, now_ms()])?;
    Ok(changed)
}
