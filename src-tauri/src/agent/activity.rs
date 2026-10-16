//! The agent activity log: one `agent_activity` row per tool call, errors included, kept for
//! 90 days. Writes are undoable as a whole.
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use crate::db::{items, new_id, now_ms, tasks};
use crate::error::{AppError, AppResult};
use crate::model::{ActivityRow, Actor};

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

/// Whether a logged call can be undone (ignoring whether it already was): a write other than
/// `export_task` that recorded at least one change, even if it failed part-way.
fn undoable(tool: &str, is_write: bool, item_events: &[String], task_events: &[String], added: &[String]) -> bool {
    let changed_nothing = item_events.is_empty() && task_events.is_empty() && added.is_empty();
    is_write && tool != "export_task" && !changed_nothing
}

/// The rows of the latest calls, newest first; `changes_only` keeps writes. `limit` is clamped
/// to 1–500.
pub fn list(conn: &Connection, changes_only: bool, limit: i64) -> AppResult<Vec<ActivityRow>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.at, c.name, a.tool, a.is_write, a.detail, a.item_event_ids, a.task_event_ids,
                a.added_item_ids, a.result_url, a.error, a.undone_at
         FROM agent_activity a JOIN mcp_clients c ON c.id = a.client_id
         WHERE a.is_write = 1 OR NOT ?1
         ORDER BY a.at DESC, a.id DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![changes_only, limit.clamp(1, 500)], |r| {
        Ok((
            ActivityRow {
                id: r.get(0)?,
                at: r.get(1)?,
                client_name: r.get(2)?,
                tool: r.get(3)?,
                is_write: r.get(4)?,
                detail: Value::Null,
                result_url: r.get(9)?,
                error: r.get(10)?,
                undone_at: r.get(11)?,
                undoable: false,
            },
            r.get::<_, String>(5)?,
            [r.get::<_, String>(6)?, r.get::<_, String>(7)?, r.get::<_, String>(8)?],
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (mut row, detail, [item_events, task_events, added]) = row?;
        row.detail = serde_json::from_str(&detail)?;
        row.undoable = undoable(&row.tool, row.is_write, &ids(&item_events)?, &ids(&task_events)?, &ids(&added)?);
        out.push(row);
    }
    Ok(out)
}

/// What [`undo_activity`] changed.
#[derive(Debug, Default, PartialEq)]
pub struct Undone {
    pub item_ids: Vec<String>,
    pub task_ids: Vec<String>,
}

/// Undoes a write call: its item and task events (newest first; ones already undone are
/// skipped), then trashes the items it added. A call that failed part-way is undoable for the
/// changes it recorded. Returns the changed item and task ids. `notFound`, `alreadyUndone`,
/// `notUndoable` (see [`undoable`]).
pub fn undo_activity(conn: &mut Connection, id: &str) -> AppResult<Undone> {
    let row: Option<(String, bool, Option<i64>, String, String, String)> = conn
        .query_row(
            "SELECT tool, is_write, undone_at, item_event_ids, task_event_ids, added_item_ids
             FROM agent_activity WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()?;
    let (tool, is_write, undone_at, item_events, task_events, added) =
        row.ok_or(AppError::Code("notFound"))?;
    if undone_at.is_some() {
        return Err(AppError::Code("alreadyUndone"));
    }
    let (item_events, task_events, added) = (ids(&item_events)?, ids(&task_events)?, ids(&added)?);
    if !undoable(&tool, is_write, &item_events, &task_events, &added) {
        return Err(AppError::Code("notUndoable"));
    }
    fn push(ids: &mut Vec<String>, id: String) {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let mut undone = Undone::default();
    for ev in item_events.iter().rev() {
        match items::undo_event(conn, ev) {
            Ok(item) => push(&mut undone.item_ids, item),
            Err(AppError::Code("alreadyUndone")) => {}
            Err(e) => return Err(e),
        }
    }
    for ev in task_events.iter().rev() {
        match tasks::undo_event(conn, ev) {
            Ok(task) => push(&mut undone.task_ids, task),
            Err(AppError::Code("alreadyUndone")) => {}
            Err(e) => return Err(e),
        }
    }
    let mut live = Vec::new();
    for item in added {
        let row = items::get_row(conn, &item)?;
        if row.deleted_at.is_none() {
            live.push(item);
        }
    }
    items::trash(conn, &live, &Actor::User)?;
    live.into_iter().for_each(|id| push(&mut undone.item_ids, id));
    conn.execute("UPDATE agent_activity SET undone_at = ?2 WHERE id = ?1", params![id, now_ms()])?;
    Ok(undone)
}
