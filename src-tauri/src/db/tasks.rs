//! Local tasks. Every write is one transaction that bumps `updated_at` and appends a
//! `task_events` row whose `before_json`/`after_json` hold only the changed columns.
use chrono::{Local, NaiveDate, NaiveTime, TimeZone};
use rusqlite::types::{Type, Value as SqlValue, ValueRef};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row, Transaction};
use serde_json::{Map, Value};
use tauri::AppHandle;

use crate::db::{items, new_id, now_ms, search, undo};
use crate::error::{AppError, AppResult};
use crate::events;
use crate::model::{Actor, NewTask, TaskDetail, TaskEvent, TaskPatch, TaskQuery, TaskSummary};

/// Columns `undo_event` may restore from `before_json`.
const TASK_UNDOABLE: [&str; 11] = [
    "title",
    "notes",
    "status",
    "due_date",
    "due_time",
    "priority",
    "estimate_min",
    "first_step",
    "snoozed_until",
    "done_at",
    "deleted_at",
];

/// `TaskSummary` columns. `source_title` is withheld once the source item is trashed/deleted;
/// `category_id` is the source item's category while both are live, else `todo`; the last
/// column is the most recently updated live `task_exports` row as a `TaskExportState` JSON.
const SUMMARY_SELECT: &str = "SELECT t.id, t.title, t.status, t.due_date, t.due_time, t.priority,
    t.estimate_min, t.first_step, t.source_item_id,
    CASE WHEN i.deleted_at IS NULL THEN i.title END,
    COALESCE(CASE WHEN i.deleted_at IS NULL THEN c.id END, 'todo'),
    t.added_by, t.snoozed_until, t.done_at, t.created_at, t.updated_at,
    (SELECT json_object('provider', e.provider, 'state', e.state, 'error', e.error,
        'externalUrl', e.external_url, 'nextAttemptAt', e.next_attempt_at)
      FROM task_exports e WHERE e.task_id = t.id AND e.deleted_at IS NULL
      ORDER BY e.updated_at DESC LIMIT 1)
  FROM tasks t
  LEFT JOIN items i ON i.id = t.source_item_id
  LEFT JOIN categories c ON c.id = i.category_id AND c.deleted_at IS NULL";

const EVENT_COLS: &str =
    "id, at, actor, client_id, action, before_json, after_json, undone_at";

const DONE_LIMIT: usize = 200;
const DUE_SOON_MS: i64 = 30 * 60 * 1000;

fn map_summary(r: &Row<'_>) -> rusqlite::Result<TaskSummary> {
    Ok(TaskSummary {
        id: r.get(0)?,
        title: r.get(1)?,
        status: r.get(2)?,
        due_date: r.get(3)?,
        due_time: r.get(4)?,
        priority: r.get(5)?,
        estimate_min: r.get(6)?,
        first_step: r.get(7)?,
        source_item_id: r.get(8)?,
        source_title: r.get(9)?,
        category_id: r.get(10)?,
        added_by: r.get(11)?,
        snoozed_until: r.get(12)?,
        done_at: r.get(13)?,
        created_at: r.get(14)?,
        updated_at: r.get(15)?,
        export: r
            .get::<_, Option<String>>(16)?
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(16, Type::Text, Box::new(e)))?,
    })
}

fn map_event(r: &Row<'_>) -> rusqlite::Result<TaskEvent> {
    Ok(TaskEvent {
        id: r.get(0)?,
        at: r.get(1)?,
        actor: r.get(2)?,
        client_id: r.get(3)?,
        action: r.get(4)?,
        before_json: r.get(5)?,
        after_json: r.get(6)?,
        undone_at: r.get(7)?,
    })
}

/// Every column of the live task `id`, keyed by name; `notFound` when missing or deleted.
fn row_map(conn: &Connection, id: &str) -> AppResult<Map<String, Value>> {
    let mut stmt = conn.prepare("SELECT * FROM tasks WHERE id = ?1 AND deleted_at IS NULL")?;
    let names: Vec<String> = stmt.column_names().into_iter().map(String::from).collect();
    let mut rows = stmt.query([id])?;
    let row = rows.next()?.ok_or(AppError::Code("notFound"))?;
    let mut obj = Map::new();
    for (i, name) in names.iter().enumerate() {
        let v = match row.get_ref(i)? {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(n) => Value::from(n),
            ValueRef::Real(f) => Value::from(f),
            ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
            ValueRef::Blob(_) => Value::Null,
        };
        obj.insert(name.clone(), v);
    }
    Ok(obj)
}

fn add_event(
    conn: &Connection,
    task_id: &str,
    actor: &Actor,
    action: &str,
    before: Option<&Value>,
    after: Option<&Value>,
    now: i64,
) -> AppResult<String> {
    let id = new_id();
    let (actor, client_id) = actor.columns();
    conn.execute(
        "INSERT INTO task_events (id, task_id, at, actor, client_id, action, before_json, after_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?3)",
        params![
            id,
            task_id,
            now,
            actor,
            client_id,
            action,
            before.map(Value::to_string),
            after.map(Value::to_string)
        ],
    )?;
    Ok(id)
}

/// Writes the columns of `changes` that differ from the live row and records one `action`
/// event. Returns `None` (and writes nothing) when nothing differs.
fn apply(
    tx: &Transaction<'_>,
    id: &str,
    actor: &Actor,
    action: &str,
    changes: Vec<(&'static str, Value)>,
    now: i64,
) -> AppResult<Option<String>> {
    let row = row_map(tx, id)?;
    let mut before = Map::new();
    let mut after = Map::new();
    for (col, value) in changes {
        if row.get(col) != Some(&value) {
            before.insert(col.into(), row.get(col).cloned().unwrap_or(Value::Null));
            after.insert(col.into(), value);
        }
    }
    if after.is_empty() {
        return Ok(None);
    }
    let mut values = vec![SqlValue::Text(id.to_owned()), SqlValue::Integer(now)];
    let mut sets = Vec::new();
    for (col, value) in &after {
        values.push(undo::json_to_sql(value));
        sets.push(format!("{col} = ?{}", values.len()));
    }
    tx.execute(
        &format!("UPDATE tasks SET {}, updated_at = ?2 WHERE id = ?1", sets.join(", ")),
        params_from_iter(values),
    )?;
    let before = Value::Object(before);
    let after = Value::Object(after);
    Ok(Some(add_event(tx, id, actor, action, Some(&before), Some(&after), now)?))
}

/// Emits `tasks-changed` for `ids`, then `counts-changed`.
pub fn notify_changed(app: &AppHandle, ids: Vec<String>) {
    events::tasks_changed(app, ids);
    items::emit_counts(app);
}

// ---------- validation ----------

fn valid_title(title: &str) -> AppResult<String> {
    let t = title.trim();
    let n = t.chars().count();
    if n == 0 || n > 300 {
        return Err(AppError::Code("invalid"));
    }
    Ok(t.to_owned())
}

/// Validates the due pair and returns it normalized (`%Y-%m-%d`, `%H:%M`).
fn validate_due(
    due_date: Option<&str>,
    due_time: Option<&str>,
) -> AppResult<(Option<String>, Option<String>)> {
    let date = due_date
        .map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").map_err(|_| AppError::Code("invalid")))
        .transpose()?;
    let time = due_time
        .map(|t| NaiveTime::parse_from_str(t, "%H:%M").map_err(|_| AppError::Code("invalid")))
        .transpose()?;
    if time.is_some() && date.is_none() {
        return Err(AppError::Code("invalid"));
    }
    Ok((
        date.map(|d| d.format("%Y-%m-%d").to_string()),
        time.map(|t| t.format("%H:%M").to_string()),
    ))
}

fn validate_priority(p: i64) -> AppResult<()> {
    if (0..=3).contains(&p) {
        Ok(())
    } else {
        Err(AppError::Code("invalid"))
    }
}

fn validate_estimate(e: Option<i64>) -> AppResult<()> {
    match e {
        Some(m) if !(1..=1440).contains(&m) => Err(AppError::Code("invalid")),
        _ => Ok(()),
    }
}

fn opt_str(v: &Option<String>) -> Value {
    v.as_ref().map_or(Value::Null, |s| Value::String(s.clone()))
}

// ---------- writes ----------

/// Creates a task (event `created`, `after_json` = the full row). Returns the task and the
/// event id.
pub fn create(
    conn: &mut Connection,
    input: &NewTask,
    actor: &Actor,
) -> AppResult<(TaskSummary, String)> {
    let title = valid_title(&input.title)?;
    let (due_date, due_time) = validate_due(input.due_date.as_deref(), input.due_time.as_deref())?;
    let priority = input.priority.unwrap_or(0);
    validate_priority(priority)?;
    validate_estimate(input.estimate_min)?;
    let tx = conn.transaction()?;
    if let Some(src) = &input.source_item_id {
        let live: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM items WHERE id = ?1 AND deleted_at IS NULL)",
            [src],
            |r| r.get(0),
        )?;
        if !live {
            return Err(AppError::Code("notFound"));
        }
    }
    let id = new_id();
    let now = now_ms();
    let (_, client_id) = actor.columns();
    tx.execute(
        "INSERT INTO tasks (id, title, notes, due_date, due_time, priority, estimate_min, first_step,
           source_item_id, added_by, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
        params![
            id,
            title,
            input.notes.as_deref().unwrap_or(""),
            due_date,
            due_time,
            priority,
            input.estimate_min,
            input.first_step,
            input.source_item_id,
            client_id,
            now
        ],
    )?;
    let after = Value::Object(row_map(&tx, &id)?);
    let event = add_event(&tx, &id, actor, "created", None, Some(&after), now)?;
    let summary = summary(&tx, &id)?;
    tx.commit()?;
    Ok((summary, event))
}

/// Applies `patch` (event `edited`). A patch that changes nothing records no event.
pub fn update(
    conn: &mut Connection,
    id: &str,
    patch: &TaskPatch,
    actor: &Actor,
) -> AppResult<Vec<String>> {
    let tx = conn.transaction()?;
    let row = row_map(&tx, id)?;
    let cur = |col: &str| row.get(col).and_then(Value::as_str).map(String::from);
    let due_date = patch.due_date.clone().unwrap_or_else(|| cur("due_date"));
    let due_time = patch.due_time.clone().unwrap_or_else(|| cur("due_time"));
    let (due_date, due_time) = validate_due(due_date.as_deref(), due_time.as_deref())?;
    let mut changes = Vec::new();
    if let Some(t) = &patch.title {
        changes.push(("title", Value::String(valid_title(t)?)));
    }
    if let Some(n) = &patch.notes {
        changes.push(("notes", Value::String(n.clone())));
    }
    if patch.due_date.is_some() {
        changes.push(("due_date", opt_str(&due_date)));
    }
    if patch.due_time.is_some() {
        changes.push(("due_time", opt_str(&due_time)));
    }
    if let Some(p) = patch.priority {
        validate_priority(p)?;
        changes.push(("priority", Value::from(p)));
    }
    if let Some(e) = patch.estimate_min {
        validate_estimate(e)?;
        changes.push(("estimate_min", e.map_or(Value::Null, Value::from)));
    }
    if let Some(f) = &patch.first_step {
        changes.push(("first_step", opt_str(f)));
    }
    let event = apply(&tx, id, actor, "edited", changes, now_ms())?;
    tx.commit()?;
    Ok(event.into_iter().collect())
}

/// Marks the task done (`done`, sets `done_at`) or open again (`reopened`, clears it); both
/// clear `snoozed_until`. A task already in the target status is left untouched (no event).
pub fn set_done(
    conn: &mut Connection,
    id: &str,
    done: bool,
    actor: &Actor,
) -> AppResult<Vec<String>> {
    let tx = conn.transaction()?;
    let status_now = row_map(&tx, id)?.get("status").cloned();
    if status_now == Some(Value::from(if done { "done" } else { "open" })) {
        return Ok(Vec::new());
    }
    let now = now_ms();
    let (status, done_at, action) = if done {
        ("done", Value::from(now), "done")
    } else {
        ("open", Value::Null, "reopened")
    };
    let changes = vec![
        ("status", Value::from(status)),
        ("done_at", done_at),
        ("snoozed_until", Value::Null),
    ];
    let event = apply(&tx, id, actor, action, changes, now)?;
    tx.commit()?;
    Ok(event.into_iter().collect())
}

pub fn snooze(conn: &mut Connection, id: &str, until_ms: i64, actor: &Actor) -> AppResult<Vec<String>> {
    let tx = conn.transaction()?;
    let changes = vec![("snoozed_until", Value::from(until_ms))];
    let event = apply(&tx, id, actor, "snoozed", changes, now_ms())?;
    tx.commit()?;
    Ok(event.into_iter().collect())
}

/// Soft delete (`trashed`, sets `deleted_at`).
pub fn trash(conn: &mut Connection, id: &str, actor: &Actor) -> AppResult<Vec<String>> {
    let tx = conn.transaction()?;
    let now = now_ms();
    let event = apply(&tx, id, actor, "trashed", vec![("deleted_at", Value::from(now))], now)?;
    tx.commit()?;
    Ok(event.into_iter().collect())
}

/// Restores the event's `before_json` onto its task (undoing `created` trashes the task) and
/// marks the event undone. Appends no new event. Returns the task id.
pub fn undo_event(conn: &mut Connection, event_id: &str) -> AppResult<String> {
    let tx = conn.transaction()?;
    let (task_id, action, before, undone_at): (String, String, Option<String>, Option<i64>) = tx
        .query_row(
            "SELECT task_id, action, before_json, undone_at FROM task_events WHERE id = ?1",
            [event_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?
        .ok_or(AppError::Code("notFound"))?;
    if undone_at.is_some() {
        return Err(AppError::Code("alreadyUndone"));
    }
    let now = now_ms();
    if action == "created" {
        tx.execute(
            "UPDATE tasks SET deleted_at = ?2, updated_at = ?2 WHERE id = ?1",
            params![task_id, now],
        )?;
    } else {
        let before: Option<Value> = before.as_deref().map(serde_json::from_str).transpose()?;
        undo::restore_before(&tx, "tasks", "id", &task_id, before.as_ref(), &TASK_UNDOABLE, now)?;
    }
    tx.execute(
        "UPDATE task_events SET undone_at = ?2, updated_at = ?2 WHERE id = ?1",
        params![event_id, now],
    )?;
    tx.commit()?;
    Ok(task_id)
}

// ---------- reads ----------

fn summary(conn: &Connection, id: &str) -> AppResult<TaskSummary> {
    conn.query_row(
        &format!("{SUMMARY_SELECT} WHERE t.id = ?1 AND t.deleted_at IS NULL"),
        [id],
        map_summary,
    )
    .optional()?
    .ok_or(AppError::Code("notFound"))
}

/// `open`: live open tasks, soonest due first (undated last). `done`: live done tasks, newest
/// first, at most 200. `search` keeps titles containing every folded token.
pub fn list(conn: &Connection, q: &TaskQuery) -> AppResult<Vec<TaskSummary>> {
    let (status, order) = match q.view.as_str() {
        "open" => (
            "open",
            "t.due_date IS NULL, t.due_date, t.due_time IS NULL, t.due_time, t.priority DESC, t.created_at",
        ),
        "done" => ("done", "t.done_at DESC, t.id DESC"),
        _ => return Err(AppError::Code("invalid")),
    };
    let mut stmt = conn.prepare(&format!(
        "{SUMMARY_SELECT} WHERE t.deleted_at IS NULL AND t.status = ?1 ORDER BY {order}"
    ))?;
    let rows = stmt.query_map([status], map_summary)?;
    let tokens: Vec<String> = q
        .search
        .as_deref()
        .map(|s| search::fold(s).split_whitespace().map(String::from).collect())
        .unwrap_or_default();
    let mut out = Vec::new();
    for row in rows {
        let task = row?;
        if !tokens.is_empty() {
            let title = search::fold(&task.title);
            if !tokens.iter().all(|t| title.contains(t.as_str())) {
                continue;
            }
        }
        out.push(task);
        if status == "done" && out.len() == DONE_LIMIT {
            break;
        }
    }
    Ok(out)
}

/// The task, its notes and its events (newest first). `notFound` for missing/deleted tasks.
pub fn get(conn: &Connection, id: &str) -> AppResult<TaskDetail> {
    let task = summary(conn, id)?;
    let notes: String = conn.query_row("SELECT notes FROM tasks WHERE id = ?1", [id], |r| r.get(0))?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {EVENT_COLS} FROM task_events WHERE task_id = ?1 ORDER BY at DESC, id DESC"
    ))?;
    let events = stmt
        .query_map([id], map_event)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(TaskDetail { task, notes, events })
}

/// Live tasks converted from `item_id`, oldest first.
pub fn for_item(conn: &Connection, item_id: &str) -> AppResult<Vec<TaskSummary>> {
    let mut stmt = conn.prepare(&format!(
        "{SUMMARY_SELECT} WHERE t.deleted_at IS NULL AND t.source_item_id = ?1 ORDER BY t.created_at, t.id"
    ))?;
    let rows = stmt.query_map([item_id], map_summary)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Live open tasks not snoozed at `now_ms` (`snoozed_until` NULL or ≤ now), unordered.
pub fn open_unsnoozed(conn: &Connection, now_ms: i64) -> AppResult<Vec<TaskSummary>> {
    let mut stmt = conn.prepare(&format!(
        "{SUMMARY_SELECT} WHERE t.deleted_at IS NULL AND t.status = 'open'
           AND (t.snoozed_until IS NULL OR t.snoozed_until <= ?1)"
    ))?;
    let rows = stmt.query_map([now_ms], map_summary)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Epoch ms of a local calendar date + `HH:MM`; `None` if unparsable or skipped by DST.
fn local_ms(date: &str, time: &str) -> Option<i64> {
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let t = NaiveTime::parse_from_str(time, "%H:%M").ok()?;
    Local
        .from_local_datetime(&d.and_time(t))
        .earliest()
        .map(|dt| dt.timestamp_millis())
}

/// `(open, done today, due soon)` at `now_ms`. Due soon: an open, unsnoozed task with a due time
/// in `[now, now + 30 min]`; tasks without `due_time` never count.
pub fn counts(conn: &Connection, now_ms: i64) -> AppResult<(i64, i64, bool)> {
    let open: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE deleted_at IS NULL AND status = 'open'",
        [],
        |r| r.get(0),
    )?;
    let midnight = Local
        .timestamp_millis_opt(now_ms)
        .single()
        .and_then(|dt| {
            Local
                .from_local_datetime(&dt.date_naive().and_time(NaiveTime::MIN))
                .earliest()
        })
        .map_or(now_ms, |dt| dt.timestamp_millis());
    let done_today: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE deleted_at IS NULL AND status = 'done' AND done_at >= ?1",
        [midnight],
        |r| r.get(0),
    )?;
    let mut stmt = conn.prepare(
        "SELECT due_date, due_time FROM tasks
         WHERE deleted_at IS NULL AND status = 'open' AND due_date IS NOT NULL AND due_time IS NOT NULL
           AND (snoozed_until IS NULL OR snoozed_until < ?1)",
    )?;
    let mut due_soon = false;
    let rows = stmt.query_map([now_ms], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows {
        let (date, time) = row?;
        if local_ms(&date, &time).is_some_and(|at| at >= now_ms && at <= now_ms + DUE_SOON_MS) {
            due_soon = true;
            break;
        }
    }
    Ok((open, done_today, due_soon))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::items::{insert_item, recategorize, trash as trash_items};
    use crate::db::Db;
    use crate::model::NewItem;

    fn db() -> Db {
        Db::open_in_memory().unwrap()
    }

    fn new_task(title: &str) -> NewTask {
        NewTask { title: title.into(), ..Default::default() }
    }

    fn item(conn: &mut Connection, title: &str) -> String {
        insert_item(
            conn,
            &NewItem {
                kind: "text".into(),
                title: title.into(),
                payload_text: Some("body".into()),
                source_kind: "drop".into(),
                content_hash: format!("h-{title}"),
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn create_and_undo_created_trashes() {
        let db = db();
        let mut conn = db.conn();
        let (task, event) = create(&mut conn, &new_task("  Write report  "), &Actor::User).unwrap();
        assert_eq!(task.title, "Write report");
        assert_eq!(task.status, "open");
        assert_eq!(task.category_id, "todo");
        let detail = get(&conn, &task.id).unwrap();
        assert_eq!(detail.events.len(), 1);
        assert_eq!(detail.events[0].id, event);
        assert_eq!(detail.events[0].action, "created");
        assert_eq!(detail.events[0].actor, "user");
        assert!(detail.events[0].before_json.is_none());
        let after: Value = serde_json::from_str(detail.events[0].after_json.as_ref().unwrap()).unwrap();
        assert_eq!(after["title"], "Write report");
        assert_eq!(after["status"], "open");

        assert_eq!(undo_event(&mut conn, &event).unwrap(), task.id);
        assert!(matches!(get(&conn, &task.id), Err(AppError::Code("notFound"))));
        assert!(list(&conn, &TaskQuery { view: "open".into(), search: None }).unwrap().is_empty());
        assert!(matches!(undo_event(&mut conn, &event), Err(AppError::Code("alreadyUndone"))));
        assert!(matches!(undo_event(&mut conn, "nope"), Err(AppError::Code("notFound"))));
    }

    #[test]
    fn edit_then_undo_restores_fields() {
        let db = db();
        let mut conn = db.conn();
        let (task, _) = create(
            &mut conn,
            &NewTask { due_date: Some("2026-10-20".into()), estimate_min: Some(10), ..new_task("Plan") },
            &Actor::User,
        )
        .unwrap();
        let noop = TaskPatch { title: Some("Plan".into()), ..Default::default() };
        assert!(update(&mut conn, &task.id, &noop, &Actor::User).unwrap().is_empty());

        let patch: TaskPatch = serde_json::from_str(
            r#"{"title":"Plan trip","dueDate":null,"priority":2,"firstStep":"Open map"}"#,
        )
        .unwrap();
        assert_eq!(patch.estimate_min, None);
        assert_eq!(patch.due_date, Some(None));
        let ev = update(&mut conn, &task.id, &patch, &Actor::User).unwrap();
        assert_eq!(ev.len(), 1);
        let d = get(&conn, &task.id).unwrap();
        assert_eq!(d.task.title, "Plan trip");
        assert_eq!(d.task.due_date, None);
        assert_eq!(d.task.priority, 2);
        assert_eq!(d.task.estimate_min, Some(10));
        assert_eq!(d.task.first_step.as_deref(), Some("Open map"));
        let before: Value = serde_json::from_str(d.events[0].before_json.as_ref().unwrap()).unwrap();
        let keys: Vec<&String> = before.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["due_date", "first_step", "priority", "title"]);

        undo_event(&mut conn, &ev[0]).unwrap();
        let d = get(&conn, &task.id).unwrap();
        assert_eq!(d.task.title, "Plan");
        assert_eq!(d.task.due_date.as_deref(), Some("2026-10-20"));
        assert_eq!(d.task.priority, 0);
        assert_eq!(d.task.first_step, None);
        assert!(d.events[0].undone_at.is_some());
    }

    #[test]
    fn done_sets_done_at_and_undo_reopens() {
        let db = db();
        let mut conn = db.conn();
        let (task, _) = create(&mut conn, &new_task("Call bank"), &Actor::User).unwrap();
        snooze(&mut conn, &task.id, now_ms() + 60_000, &Actor::User).unwrap();
        let ev = set_done(&mut conn, &task.id, true, &Actor::User).unwrap();
        let t = get(&conn, &task.id).unwrap().task;
        assert_eq!(t.status, "done");
        assert!(t.done_at.is_some());
        assert_eq!(t.snoozed_until, None);
        assert_eq!(counts(&conn, now_ms()).unwrap().1, 1);

        undo_event(&mut conn, &ev[0]).unwrap();
        let t = get(&conn, &task.id).unwrap().task;
        assert_eq!(t.status, "open");
        assert_eq!(t.done_at, None);
        assert!(t.snoozed_until.is_some());

        let ev = set_done(&mut conn, &task.id, true, &Actor::User).unwrap();
        let ev2 = set_done(&mut conn, &task.id, false, &Actor::User).unwrap();
        assert_eq!(get(&conn, &task.id).unwrap().events[0].action, "reopened");
        assert_ne!(ev, ev2);
        assert_eq!(get(&conn, &task.id).unwrap().task.done_at, None);
    }

    #[test]
    fn set_done_to_current_status_is_noop() {
        let db = db();
        let mut conn = db.conn();
        let (task, _) = create(&mut conn, &new_task("Pay rent"), &Actor::User).unwrap();
        let until = now_ms() + 60_000;
        snooze(&mut conn, &task.id, until, &Actor::User).unwrap();
        assert!(set_done(&mut conn, &task.id, false, &Actor::User).unwrap().is_empty());
        assert_eq!(get(&conn, &task.id).unwrap().task.snoozed_until, Some(until));

        set_done(&mut conn, &task.id, true, &Actor::User).unwrap();
        let done_at = get(&conn, &task.id).unwrap().task.done_at;
        let events = get(&conn, &task.id).unwrap().events.len();
        assert!(set_done(&mut conn, &task.id, true, &Actor::User).unwrap().is_empty());
        let d = get(&conn, &task.id).unwrap();
        assert_eq!(d.task.done_at, done_at);
        assert_eq!(d.events.len(), events);
    }

    #[test]
    fn due_values_are_normalized() {
        let db = db();
        let mut conn = db.conn();
        let input = NewTask {
            due_date: Some("2026-1-5".into()),
            due_time: Some("9:00".into()),
            ..new_task("Dentist")
        };
        let (task, _) = create(&mut conn, &input, &Actor::User).unwrap();
        assert_eq!(task.due_date.as_deref(), Some("2026-01-05"));
        assert_eq!(task.due_time.as_deref(), Some("09:00"));
        let patch = TaskPatch {
            due_date: Some(Some("2026-2-3".into())),
            due_time: Some(Some("7:05".into())),
            ..Default::default()
        };
        update(&mut conn, &task.id, &patch, &Actor::User).unwrap();
        let t = get(&conn, &task.id).unwrap().task;
        assert_eq!(t.due_date.as_deref(), Some("2026-02-03"));
        assert_eq!(t.due_time.as_deref(), Some("07:05"));
    }

    #[test]
    fn due_soon_needs_time_within_30_min() {
        let db = db();
        let mut conn = db.conn();
        let base = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
        let at = |h, m| {
            Local
                .from_local_datetime(&base.and_hms_opt(h, m, 0).unwrap())
                .earliest()
                .unwrap()
                .timestamp_millis()
        };
        let now = at(9, 0);
        let mk = |conn: &mut Connection, date: &str, time: Option<&str>| {
            create(
                conn,
                &NewTask {
                    due_date: Some(date.into()),
                    due_time: time.map(String::from),
                    ..new_task("Task")
                },
                &Actor::User,
            )
            .unwrap()
            .0
        };
        mk(&mut conn, "2026-10-16", None);
        mk(&mut conn, "2026-10-16", Some("09:31"));
        mk(&mut conn, "2026-10-16", Some("08:59"));
        assert_eq!(counts(&conn, now).unwrap(), (3, 0, false));

        let soon = mk(&mut conn, "2026-10-16", Some("09:30"));
        assert!(counts(&conn, now).unwrap().2);
        snooze(&mut conn, &soon.id, now + 60_000, &Actor::User).unwrap();
        assert!(!counts(&conn, now).unwrap().2);
        assert!(counts(&conn, now + 120_000).unwrap().2);
    }

    #[test]
    fn invalid_due_time_without_date() {
        let db = db();
        let mut conn = db.conn();
        let mut bad = |t: NewTask| matches!(create(&mut conn, &t, &Actor::User), Err(AppError::Code("invalid")));
        assert!(bad(NewTask { due_time: Some("09:00".into()), ..new_task("A") }));
        assert!(bad(NewTask { due_date: Some("2026-13-01".into()), ..new_task("A") }));
        assert!(bad(NewTask { due_date: Some("2026-10-01".into()), due_time: Some("25:00".into()), ..new_task("A") }));
        assert!(bad(new_task("   ")));
        assert!(bad(new_task(&"x".repeat(301))));
        assert!(bad(NewTask { priority: Some(4), ..new_task("A") }));
        assert!(bad(NewTask { estimate_min: Some(0), ..new_task("A") }));
        assert!(bad(NewTask { estimate_min: Some(1441), ..new_task("A") }));
        assert!(matches!(
            create(&mut conn, &NewTask { source_item_id: Some("missing".into()), ..new_task("A") }, &Actor::User),
            Err(AppError::Code("notFound"))
        ));

        let (task, _) = create(
            &mut conn,
            &NewTask { due_date: Some("2026-10-01".into()), due_time: Some("09:00".into()), ..new_task("A") },
            &Actor::User,
        )
        .unwrap();
        let clear_date: TaskPatch = serde_json::from_str(r#"{"dueDate":null}"#).unwrap();
        assert!(matches!(
            update(&mut conn, &task.id, &clear_date, &Actor::User),
            Err(AppError::Code("invalid"))
        ));
    }

    #[test]
    fn list_done_orders_by_done_at() {
        let db = db();
        let mut conn = db.conn();
        let mut ids = Vec::new();
        for title in ["Đọc sách", "Buy milk", "Read book"] {
            let (t, _) = create(&mut conn, &new_task(title), &Actor::User).unwrap();
            set_done(&mut conn, &t.id, true, &Actor::User).unwrap();
            ids.push(t.id);
        }
        // Spread done_at so the order is not decided by insertion time.
        for (i, id) in ids.iter().enumerate() {
            conn.execute("UPDATE tasks SET done_at = ?2 WHERE id = ?1", params![id, 1000 - i as i64 * 100])
                .unwrap();
        }
        conn.execute("UPDATE tasks SET done_at = 5000 WHERE id = ?1", [&ids[2]]).unwrap();
        let done = list(&conn, &TaskQuery { view: "done".into(), search: None }).unwrap();
        let got: Vec<&str> = done.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(got, [ids[2].as_str(), ids[0].as_str(), ids[1].as_str()]);
        assert!(list(&conn, &TaskQuery { view: "open".into(), search: None }).unwrap().is_empty());

        let found = list(&conn, &TaskQuery { view: "done".into(), search: Some("doc".into()) }).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, ids[0]);
        let found = list(&conn, &TaskQuery { view: "done".into(), search: Some("re bo".into()) }).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, ids[2]);
    }

    #[test]
    fn category_follows_source_item() {
        let db = db();
        let mut conn = db.conn();
        let item_id = item(&mut conn, "Paper");
        recategorize(&mut conn, &[item_id.clone()], "read", "user").unwrap();
        let (task, _) = create(
            &mut conn,
            &NewTask { source_item_id: Some(item_id.clone()), ..new_task("Read paper") },
            &Actor::User,
        )
        .unwrap();
        assert_eq!(task.category_id, "read");
        assert_eq!(task.source_title.as_deref(), Some("Paper"));
        assert_eq!(for_item(&conn, &item_id).unwrap().len(), 1);

        recategorize(&mut conn, &[item_id.clone()], "code", "user").unwrap();
        assert_eq!(get(&conn, &task.id).unwrap().task.category_id, "code");

        trash_items(&mut conn, &[item_id.clone()]).unwrap();
        let t = get(&conn, &task.id).unwrap().task;
        assert_eq!(t.category_id, "todo");
        assert_eq!(t.source_title, None);
    }
}
