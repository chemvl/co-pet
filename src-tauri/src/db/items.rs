//! Item reads and writes. Every write: one transaction, `updated_at`, FTS row, `item_events`.
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row, Transaction};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::db::{categories, new_id, now_ms, search, tasks, undo, Db};
use crate::error::{AppError, AppResult};
use crate::events;
use crate::model::{
    Actor, Counts, Cursor, ItemDetail, ItemEvent, ItemPage, ItemQuery, ItemRow, ItemSummary,
    NewItem, TriageUpdate,
};
use crate::paths::Paths;

/// Every `items` column, in the order `map_row` reads them.
const COLS: &str = "id, kind, title, summary, tags, payload_text, payload_html, url, file_path, \
    file_name, mime, size_bytes, thumb_path, source_app, source_bundle_id, source_kind, \
    extracted_text, extract_status, category_id, triage_status, triage_source, triage_confidence, \
    triage_reason, suggestions, sensitive, status, pinned, content_hash, captured_at, updated_at, \
    deleted_at, added_by, task_suggestion";

/// Same shape as `COLS` without the large text columns (lists never show them).
const LIST_COLS: &str = "id, kind, title, summary, tags, NULL, NULL, url, file_path, \
    file_name, mime, size_bytes, thumb_path, source_app, source_bundle_id, source_kind, \
    NULL, extract_status, category_id, triage_status, triage_source, triage_confidence, \
    triage_reason, suggestions, sensitive, status, pinned, content_hash, captured_at, updated_at, \
    deleted_at, added_by, task_suggestion";

/// Columns `undo_event` may restore from `before_json`.
const UNDOABLE: [&str; 7] = [
    "category_id",
    "triage_source",
    "triage_reason",
    "pinned",
    "status",
    "deleted_at",
    "tags",
];

/// The items agents may see (MCP): live, not trashed, not sensitive, triaged (a just-captured
/// secret is not flagged yet) and in a live category not hidden from agents. Columns are
/// qualified with `items.`; hidden ids behave like missing ones.
pub const AGENT_VISIBLE: &str = "items.deleted_at IS NULL AND items.status != 'trashed' \
    AND items.sensitive = 0 AND items.triage_status = 'done' \
    AND items.category_id IN (SELECT id FROM categories WHERE deleted_at IS NULL AND exclude_from_mcp = 0)";

const PAGE_MAX: i64 = 500;

fn json_list(s: String) -> Vec<String> {
    serde_json::from_str(&s).unwrap_or_default()
}

fn map_row(r: &Row<'_>) -> rusqlite::Result<ItemRow> {
    Ok(ItemRow {
        id: r.get(0)?,
        kind: r.get(1)?,
        title: r.get(2)?,
        summary: r.get(3)?,
        tags: json_list(r.get(4)?),
        payload_text: r.get(5)?,
        payload_html: r.get(6)?,
        url: r.get(7)?,
        file_path: r.get(8)?,
        file_name: r.get(9)?,
        mime: r.get(10)?,
        size_bytes: r.get(11)?,
        thumb_path: r.get(12)?,
        source_app: r.get(13)?,
        source_bundle_id: r.get(14)?,
        source_kind: r.get(15)?,
        extracted_text: r.get(16)?,
        extract_status: r.get(17)?,
        category_id: r.get(18)?,
        triage_status: r.get(19)?,
        triage_source: r.get(20)?,
        triage_confidence: r.get(21)?,
        triage_reason: r.get(22)?,
        suggestions: json_list(r.get(23)?),
        sensitive: r.get(24)?,
        status: r.get(25)?,
        pinned: r.get(26)?,
        captured_at: r.get(28)?,
        updated_at: r.get(29)?,
        deleted_at: r.get(30)?,
        added_by: r.get(31)?,
        task_suggestion: r.get(32)?,
    })
}

fn map_event(r: &Row<'_>) -> rusqlite::Result<ItemEvent> {
    let parse = |s: Option<String>| s.and_then(|s| serde_json::from_str(&s).ok());
    Ok(ItemEvent {
        id: r.get(0)?,
        item_id: r.get(1)?,
        at: r.get(2)?,
        actor: r.get(3)?,
        client_id: r.get(4)?,
        action: r.get(5)?,
        before_json: parse(r.get(6)?),
        after_json: parse(r.get(7)?),
        undone_at: r.get(8)?,
    })
}

/// `who`: the `(actor, client_id)` column values.
fn add_event(
    conn: &Connection,
    item_id: &str,
    who: (&str, Option<&str>),
    action: &str,
    before: Option<&Value>,
    after: Option<&Value>,
    now: i64,
) -> AppResult<String> {
    let id = new_id();
    let (actor, client_id) = who;
    conn.execute(
        "INSERT INTO item_events (id, item_id, at, actor, client_id, action, before_json, after_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?3)",
        params![
            id,
            item_id,
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

/// Re-reads the row and rewrites its FTS entry.
fn reindex(conn: &Connection, id: &str) -> AppResult<ItemRow> {
    let row = get_row(conn, id)?;
    search::index_item(conn, &row)?;
    Ok(row)
}

fn triage_state(row: &ItemRow) -> Value {
    json!({
        "category_id": row.category_id,
        "triage_source": row.triage_source,
        "triage_reason": row.triage_reason,
    })
}

fn check_category(conn: &Connection, category_id: &str) -> AppResult<()> {
    if categories::exists(conn, category_id)? {
        Ok(())
    } else {
        Err(AppError::Code("notFound"))
    }
}

// ---------- writes ----------

/// Inserts a captured item; the `captured` event's actor is the agent of `added_by`, else the
/// user.
pub fn insert_item(conn: &mut Connection, new: &NewItem) -> AppResult<String> {
    let tx = conn.transaction()?;
    let id = new_id();
    let now = now_ms();
    tx.execute(
        "INSERT INTO items (id, kind, title, payload_text, payload_html, url, file_path, file_name,
           mime, size_bytes, thumb_path, source_app, source_bundle_id, source_kind, content_hash,
           added_by, captured_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?17)",
        params![
            id,
            new.kind,
            new.title,
            new.payload_text,
            new.payload_html,
            new.url,
            new.file_path,
            new.file_name,
            new.mime,
            new.size_bytes,
            new.thumb_path,
            new.source_app,
            new.source_bundle_id,
            new.source_kind,
            new.content_hash,
            new.added_by,
            now
        ],
    )?;
    reindex(&tx, &id)?;
    let who = match &new.added_by {
        Some(client) => ("agent", Some(client.as_str())),
        None => ("user", None),
    };
    add_event(&tx, &id, who, "captured", None, None, now)?;
    tx.commit()?;
    Ok(id)
}

/// Writes the triage result. `None` fields keep their current value; `triage_status` becomes
/// `done` and a `recategorize` event (actor `triage`) records the category change.
pub fn update_triage(conn: &mut Connection, id: &str, u: &TriageUpdate) -> AppResult<()> {
    check_category(conn, &u.category_id)?;
    let tx = conn.transaction()?;
    let before = triage_state(&get_row(&tx, id)?);
    let now = now_ms();
    let tags = u.tags.as_ref().map(serde_json::to_string).transpose()?;
    tx.execute(
        "UPDATE items SET
           title = COALESCE(?2, title), summary = COALESCE(?3, summary), tags = COALESCE(?4, tags),
           extracted_text = COALESCE(?5, extracted_text), extract_status = ?6,
           thumb_path = COALESCE(?7, thumb_path), category_id = ?8, triage_status = 'done',
           triage_source = ?9, triage_confidence = ?10, triage_reason = ?11, suggestions = ?12,
           sensitive = ?13, updated_at = ?14, task_suggestion = ?15
         WHERE id = ?1",
        params![
            id,
            u.title,
            u.summary,
            tags,
            u.extracted_text,
            u.extract_status,
            u.thumb_path,
            u.category_id,
            u.triage_source,
            u.triage_confidence,
            u.triage_reason,
            serde_json::to_string(&u.suggestions)?,
            u.sensitive,
            now,
            u.task_suggestion
        ],
    )?;
    let after = triage_state(&reindex(&tx, id)?);
    add_event(
        &tx,
        id,
        ("triage", None),
        "recategorize",
        Some(&before),
        Some(&after),
        now,
    )?;
    tx.commit()?;
    Ok(())
}

/// Moves items to `category_id`. A `user` move marks the source/reason `user` and records a
/// `category_examples` row that later teaches the local model. Returns the event ids.
pub fn recategorize(
    conn: &mut Connection,
    ids: &[String],
    category_id: &str,
    actor: &Actor,
) -> AppResult<Vec<String>> {
    let tx = conn.transaction()?;
    let events = recategorize_in(&tx, ids, category_id, actor, *actor == Actor::User)?;
    tx.commit()?;
    Ok(events)
}

/// `recategorize` inside the caller's transaction. A `user` move marks the source/reason
/// `user`; an agent move marks the reason `agent`; `teach` also records a
/// `category_examples` row per non-sensitive item.
pub fn recategorize_in(
    tx: &Transaction,
    ids: &[String],
    category_id: &str,
    actor: &Actor,
    teach: bool,
) -> AppResult<Vec<String>> {
    let who = (actor.item_actor()?, actor.client_id());
    check_category(tx, category_id)?;
    let now = now_ms();
    let mut events = Vec::with_capacity(ids.len());
    for id in ids {
        let row = get_row(tx, id)?;
        let before = triage_state(&row);
        if *actor == Actor::User {
            tx.execute(
                "UPDATE items SET category_id = ?2, triage_status = 'done', triage_source = 'user',
                   triage_reason = 'user', updated_at = ?3 WHERE id = ?1",
                params![id, category_id, now],
            )?;
        } else {
            // The old reason would claim the model or a rule chose this category.
            tx.execute(
                "UPDATE items SET category_id = ?2, triage_reason = 'agent', updated_at = ?3 WHERE id = ?1",
                params![id, category_id, now],
            )?;
        }
        // Sensitive titles must never reach the model as few-shot examples.
        if teach && !row.sensitive {
            tx.execute(
                "INSERT INTO category_examples (id, item_id, category_id, title, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![new_id(), id, category_id, row.title, now],
            )?;
        }
        let after = triage_state(&reindex(tx, id)?);
        events.push(add_event(
            tx,
            id,
            who,
            "recategorize",
            Some(&before),
            Some(&after),
            now,
        )?);
    }
    Ok(events)
}

/// Replaces the item's tags (event `tag`, before/after `{tags}`). Returns the event id.
pub fn set_tags(conn: &mut Connection, id: &str, tags: &[String], actor: &Actor) -> AppResult<String> {
    let who = (actor.item_actor()?, actor.client_id());
    let tx = conn.transaction()?;
    let row = get_row(&tx, id)?;
    let now = now_ms();
    tx.execute(
        "UPDATE items SET tags = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, serde_json::to_string(tags)?, now],
    )?;
    reindex(&tx, id)?;
    let event = add_event(
        &tx,
        id,
        who,
        "tag",
        Some(&json!({ "tags": row.tags })),
        Some(&json!({ "tags": tags })),
        now,
    )?;
    tx.commit()?;
    Ok(event)
}

pub fn set_pinned(conn: &mut Connection, id: &str, pinned: bool) -> AppResult<String> {
    let tx = conn.transaction()?;
    let row = get_row(&tx, id)?;
    let now = now_ms();
    tx.execute(
        "UPDATE items SET pinned = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, pinned, now],
    )?;
    reindex(&tx, id)?;
    let action = if pinned { "pin" } else { "unpin" };
    let event = add_event(
        &tx,
        id,
        ("user", None),
        action,
        Some(&json!({ "pinned": row.pinned })),
        Some(&json!({ "pinned": pinned })),
        now,
    )?;
    tx.commit()?;
    Ok(event)
}

/// Sets `status` (and `deleted_at`) on each item, recording `action` per item.
fn set_status(
    conn: &mut Connection,
    ids: &[String],
    status: &str,
    deleted: bool,
    action: &str,
    actor: &Actor,
) -> AppResult<Vec<String>> {
    let who = (actor.item_actor()?, actor.client_id());
    let tx = conn.transaction()?;
    let now = now_ms();
    let deleted_at = deleted.then_some(now);
    let mut events = Vec::with_capacity(ids.len());
    for id in ids {
        let row = get_row(&tx, id)?;
        tx.execute(
            "UPDATE items SET status = ?2, deleted_at = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, status, deleted_at, now],
        )?;
        reindex(&tx, id)?;
        let before = json!({ "status": row.status, "deleted_at": row.deleted_at });
        let after = json!({ "status": status, "deleted_at": deleted_at });
        events.push(add_event(
            &tx,
            id,
            who,
            action,
            Some(&before),
            Some(&after),
            now,
        )?);
    }
    tx.commit()?;
    Ok(events)
}

pub fn archive(conn: &mut Connection, ids: &[String], actor: &Actor) -> AppResult<Vec<String>> {
    set_status(conn, ids, "archived", false, "archive", actor)
}

/// Archived items back to the inbox.
pub fn unarchive(conn: &mut Connection, ids: &[String], actor: &Actor) -> AppResult<Vec<String>> {
    set_status(conn, ids, "inbox", false, "unarchive", actor)
}

/// Soft delete: status `trashed` + `deleted_at`. The blob stays so Undo can restore it.
pub fn trash(conn: &mut Connection, ids: &[String], actor: &Actor) -> AppResult<Vec<String>> {
    set_status(conn, ids, "trashed", true, "trash", actor)
}

/// Restores the event's `before_json` onto its item and marks the event undone. Appends no new
/// event. Undoing a user recategorize also retracts the `category_examples` row it taught.
/// Returns the item id.
pub fn undo_event(conn: &mut Connection, event_id: &str) -> AppResult<String> {
    let tx = conn.transaction()?;
    let ev = tx
        .query_row(
            "SELECT id, item_id, at, actor, client_id, action, before_json, after_json, undone_at
             FROM item_events WHERE id = ?1",
            [event_id],
            map_event,
        )
        .optional()?
        .ok_or(AppError::Code("notFound"))?;
    if ev.undone_at.is_some() {
        return Err(AppError::Code("alreadyUndone"));
    }
    let now = now_ms();
    undo::restore_before(
        &tx,
        "items",
        "id",
        &ev.item_id,
        ev.before_json.as_ref(),
        &UNDOABLE,
        now,
    )?;
    if ev.action == "recategorize" && ev.actor == "user" {
        if let Some(cat) = ev
            .after_json
            .as_ref()
            .and_then(|a| a["category_id"].as_str())
        {
            tx.execute(
                "UPDATE category_examples SET deleted_at = ?3, updated_at = ?3
                 WHERE item_id = ?1 AND category_id = ?2 AND deleted_at IS NULL",
                params![ev.item_id, cat, now],
            )?;
        }
    }
    tx.execute(
        "UPDATE item_events SET undone_at = ?2, updated_at = ?2 WHERE id = ?1",
        params![event_id, now],
    )?;
    reindex(&tx, &ev.item_id)?;
    tx.commit()?;
    Ok(ev.item_id)
}

// ---------- reads ----------

pub fn get_row(conn: &Connection, id: &str) -> AppResult<ItemRow> {
    conn.query_row(
        &format!("SELECT {COLS} FROM items WHERE id = ?1"),
        [id],
        map_row,
    )
    .optional()?
    .ok_or(AppError::Code("notFound"))
}

/// Rows in the order of `ids`; unknown ids are an error.
pub fn get_rows(conn: &Connection, ids: &[String]) -> AppResult<Vec<ItemRow>> {
    ids.iter().map(|id| get_row(conn, id)).collect()
}

/// Whether agents may see item `id` (`AGENT_VISIBLE`).
pub fn agent_visible(conn: &Connection, id: &str) -> AppResult<bool> {
    Ok(conn.query_row(
        &format!("SELECT EXISTS (SELECT 1 FROM items WHERE items.id = ?1 AND {AGENT_VISIBLE})"),
        [id],
        |r| r.get(0),
    )?)
}

/// A live (not trashed, not deleted) item with this content hash.
pub fn find_live_by_hash(conn: &Connection, hash: &str) -> AppResult<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT id FROM items WHERE content_hash = ?1 AND deleted_at IS NULL
               AND status != 'trashed' ORDER BY captured_at DESC LIMIT 1",
            [hash],
            |r| r.get(0),
        )
        .optional()?)
}

/// Whether any item still references the blob. Trashed rows count: Undo can restore them.
pub fn file_path_in_use(conn: &Connection, rel_path: &str) -> AppResult<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM items WHERE file_path = ?1 OR thumb_path = ?1)",
        [rel_path],
        |r| r.get(0),
    )?)
}

/// Items still waiting for triage, oldest first.
pub fn pending_ids(conn: &Connection) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM items WHERE triage_status = 'pending' AND deleted_at IS NULL
         ORDER BY captured_at, id",
    )?;
    let ids = stmt
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(ids)
}

fn abs_string(paths: &Paths, rel: &str) -> String {
    paths.abs(rel).to_string_lossy().into_owned()
}

fn to_summary(conn: &Connection, row: ItemRow, paths: &Paths) -> AppResult<ItemSummary> {
    let sensitive = row.sensitive;
    let file_available = row.file_path.as_deref().is_none_or(|rel| paths.abs(rel).exists());
    let file_too_large = match row.file_path.as_deref() {
        Some(rel) => conn
            .prepare_cached("SELECT EXISTS (SELECT 1 FROM sync_blobs WHERE rel = ?1 AND state = 'tooLarge')")?
            .query_row([rel], |r| r.get(0))?,
        None => false,
    };
    Ok(ItemSummary {
        file_available,
        file_too_large,
        summary: if sensitive { None } else { row.summary },
        thumb_path: if sensitive {
            None
        } else {
            row.thumb_path.map(|t| abs_string(paths, &t))
        },
        id: row.id,
        kind: row.kind,
        title: row.title,
        tags: row.tags,
        url: row.url,
        file_name: row.file_name,
        mime: row.mime,
        size_bytes: row.size_bytes,
        source_app: row.source_app,
        source_kind: row.source_kind,
        category_id: row.category_id,
        triage_status: row.triage_status,
        triage_source: row.triage_source,
        triage_confidence: row.triage_confidence,
        triage_reason: row.triage_reason,
        suggestions: row.suggestions,
        sensitive,
        status: row.status,
        pinned: row.pinned,
        extract_status: row.extract_status,
        captured_at: row.captured_at,
        updated_at: row.updated_at,
        added_by: row.added_by,
        task_suggestion: row.task_suggestion,
    })
}

/// Keyset-paginated list by `(captured_at, id)`.
pub fn list_items(conn: &Connection, q: &ItemQuery, paths: &Paths) -> AppResult<ItemPage> {
    let mut sql =
        format!("SELECT {LIST_COLS} FROM items WHERE deleted_at IS NULL AND status = 'inbox'");
    let mut args: Vec<SqlValue> = Vec::new();
    match q.view.as_str() {
        "all" | "recent" => {}
        "pinned" => sql.push_str(" AND pinned = 1"),
        "unsorted" => sql.push_str(" AND category_id = 'unsorted' AND triage_status = 'done'"),
        other => return Err(AppError::msg(format!("unknown view {other}"))),
    }
    if let Some(cat) = &q.category_id {
        args.push(SqlValue::Text(cat.clone()));
        sql.push_str(&format!(" AND category_id = ?{}", args.len()));
    }
    if q.agent_visible {
        sql.push_str(&format!(" AND {AGENT_VISIBLE}"));
    }
    if let Some(since) = q.captured_since {
        args.push(SqlValue::Integer(since));
        sql.push_str(&format!(" AND captured_at >= ?{}", args.len()));
    }
    if let Some(m) = q.search.as_deref().and_then(search::search_query) {
        args.push(SqlValue::Text(m));
        sql.push_str(&format!(
            " AND id IN (SELECT item_id FROM items_fts WHERE items_fts MATCH ?{})",
            args.len()
        ));
    }
    let oldest = q.sort.as_deref() == Some("oldest");
    let (cmp, dir) = if oldest { (">", "ASC") } else { ("<", "DESC") };
    if let Some(c) = &q.cursor {
        args.push(SqlValue::Integer(c.captured_at));
        let at = args.len();
        args.push(SqlValue::Text(c.id.clone()));
        let id = args.len();
        sql.push_str(&format!(
            " AND (captured_at {cmp} ?{at} OR (captured_at = ?{at} AND id {cmp} ?{id}))"
        ));
    }
    let limit = q.limit.clamp(1, PAGE_MAX);
    args.push(SqlValue::Integer(limit + 1));
    sql.push_str(&format!(
        " ORDER BY captured_at {dir}, id {dir} LIMIT ?{}",
        args.len()
    ));

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt
        .query_map(params_from_iter(args), map_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let next_cursor = if rows.len() as i64 > limit {
        rows.truncate(limit as usize);
        rows.last().map(|r| Cursor {
            captured_at: r.captured_at,
            id: r.id.clone(),
        })
    } else {
        None
    };
    Ok(ItemPage {
        items: rows.into_iter().map(|r| to_summary(conn, r, paths)).collect::<AppResult<_>>()?,
        next_cursor,
    })
}

/// The item plus its events (newest first). Payload fields are withheld for sensitive items.
pub fn get_item_detail(conn: &Connection, id: &str, paths: &Paths) -> AppResult<ItemDetail> {
    let mut row = get_row(conn, id)?;
    let mut stmt = conn.prepare(
        "SELECT id, item_id, at, actor, client_id, action, before_json, after_json, undone_at
         FROM item_events WHERE item_id = ?1 ORDER BY at DESC, id DESC",
    )?;
    let events = stmt
        .query_map([id], map_event)?
        .collect::<rusqlite::Result<_>>()?;
    let (payload_text, payload_html, extracted_text) = if row.sensitive {
        (None, None, None)
    } else {
        (
            row.payload_text.take(),
            row.payload_html.take(),
            row.extracted_text.take(),
        )
    };
    let file_path = row.file_path.as_deref().map(|p| abs_string(paths, p));
    Ok(ItemDetail {
        item: to_summary(conn, row, paths)?,
        payload_text,
        payload_html,
        extracted_text,
        file_path,
        events,
        tasks: tasks::for_item(conn, id)?,
    })
}

/// Inbox counts. `unsorted` (and `by_category["unsorted"]`) uses the Unsorted-view definition:
/// triaged items left in `unsorted`, not ones still waiting for triage.
pub fn counts(conn: &Connection) -> AppResult<Counts> {
    let mut c = Counts::default();
    for cat in categories::list(conn)? {
        c.by_category.insert(cat.id, 0);
    }
    let mut stmt = conn.prepare(
        "SELECT category_id, COUNT(*), SUM(pinned),
           SUM(category_id = 'unsorted' AND triage_status = 'done')
         FROM items WHERE deleted_at IS NULL AND status = 'inbox' GROUP BY category_id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })?;
    for row in rows {
        let (cat, n, pinned, unsorted) = row?;
        c.inbox += n;
        c.pinned += pinned;
        c.unsorted += unsorted;
        c.by_category.insert(cat, n);
    }
    c.by_category.insert("unsorted".into(), c.unsorted);
    (c.tasks_open, c.tasks_done_today, c.task_due_soon) = tasks::counts(conn, now_ms())?;
    Ok(c)
}

/// Emits `items-changed {ids}` then `counts-changed` with fresh counts, and wakes sync.
pub fn notify_changed(app: &AppHandle, ids: Vec<String>) {
    events::items_changed(app, ids);
    emit_counts(app);
    crate::sync::poke(app);
}

/// Emits `counts-changed` with fresh counts; a failure to read them is logged.
pub fn emit_counts(app: &AppHandle) {
    match counts(&app.state::<Db>().conn()) {
        Ok(c) => events::counts_changed(app, c),
        Err(e) => eprintln!("[counts] read failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations::MIGRATIONS;

    fn db() -> Db {
        Db::open_in_memory().unwrap()
    }

    fn new_text(title: &str, body: &str, hash: &str) -> NewItem {
        NewItem {
            kind: "text".into(),
            title: title.into(),
            payload_text: Some(body.into()),
            source_kind: "drop".into(),
            content_hash: hash.into(),
            ..Default::default()
        }
    }

    fn query(search: &str) -> ItemQuery {
        ItemQuery {
            view: "all".into(),
            category_id: None,
            search: Some(search.into()),
            sort: None,
            limit: 50,
            cursor: None,
            agent_visible: false,
            captured_since: None,
        }
    }

    fn found(conn: &Connection, search: &str) -> Vec<String> {
        let paths = Paths::new("/tmp/copet-test".into());
        list_items(conn, &query(search), &paths)
            .unwrap()
            .items
            .into_iter()
            .map(|i| i.id)
            .collect()
    }

    fn sensitive_update(category: &str) -> TriageUpdate {
        TriageUpdate {
            extract_status: "skipped".into(),
            category_id: category.into(),
            triage_source: "none".into(),
            triage_reason: "none:sensitive".into(),
            sensitive: true,
            ..Default::default()
        }
    }

    #[test]
    fn migrations_validate() {
        MIGRATIONS.validate().unwrap();
    }

    #[test]
    fn v2_preserves_item_events() {
        let mut c = Connection::open_in_memory().unwrap();
        MIGRATIONS.to_version(&mut c, 1).unwrap();
        c.execute_batch(
            "INSERT INTO items (id, kind, title, source_kind, content_hash, captured_at, updated_at)
               VALUES ('i1', 'text', 'Note', 'drop', 'h', 1, 1);
             INSERT INTO item_events (id, item_id, at, actor, action, before_json, after_json, undone_at, updated_at)
               VALUES ('e1', 'i1', 5, 'user', 'pin', '{\"pinned\":0}', '{\"pinned\":1}', NULL, 6);",
        )
        .unwrap();
        MIGRATIONS.to_latest(&mut c).unwrap();
        let row: (String, i64, String, Option<String>, String, String, String, Option<i64>, i64) = c
            .query_row(
                "SELECT item_id, at, actor, client_id, action, before_json, after_json, undone_at, updated_at
                   FROM item_events WHERE id = 'e1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)),
            )
            .unwrap();
        assert_eq!(
            row,
            ("i1".into(), 5, "user".into(), None, "pin".into(), "{\"pinned\":0}".into(), "{\"pinned\":1}".into(), None, 6)
        );
        let insert = |id: &str, actor: &str| {
            c.execute(
                "INSERT INTO item_events (id, item_id, at, actor, client_id, action, updated_at)
                   VALUES (?1, 'i1', 7, ?2, 'client-1', 'tag', 7)",
                rusqlite::params![id, actor],
            )
        };
        assert!(insert("e2", "agent").is_ok());
        assert!(insert("e3", "robot").is_err());
    }

    #[test]
    fn fts_folds_vietnamese() {
        let db = db();
        let mut conn = db.conn();
        let id = insert_item(&mut conn, &new_text("Đường đến Việt Nam", "", "h1")).unwrap();
        insert_item(&mut conn, &new_text("Something else", "", "h2")).unwrap();
        assert_eq!(found(&conn, "duong"), vec![id.clone()]);
        assert_eq!(found(&conn, "viet nam"), vec![id.clone()]);
        assert_eq!(found(&conn, "đường"), vec![id]);
    }

    #[test]
    fn sensitive_item_found_by_title_not_body() {
        let db = db();
        let mut conn = db.conn();
        let id = insert_item(
            &mut conn,
            &new_text("Deploy notes", "token zebracorn", "h1"),
        )
        .unwrap();
        assert_eq!(found(&conn, "zebracorn"), vec![id.clone()]);
        update_triage(&mut conn, &id, &sensitive_update("unsorted")).unwrap();
        assert!(found(&conn, "zebracorn").is_empty());
        assert_eq!(found(&conn, "deploy"), vec![id.clone()]);

        let paths = Paths::new("/tmp/copet-test".into());
        let detail = get_item_detail(&conn, &id, &paths).unwrap();
        assert!(detail.payload_text.is_none());
    }

    #[test]
    fn undo_restores_previous_category() {
        let db = db();
        let mut conn = db.conn();
        let id = insert_item(&mut conn, &new_text("Note", "", "h1")).unwrap();
        let mut u = sensitive_update("unsorted");
        u.sensitive = false;
        u.triage_reason = "none:no-model".into();
        update_triage(&mut conn, &id, &u).unwrap();
        let events = recategorize(&mut conn, &[id.clone()], "read", &Actor::User).unwrap();
        assert_eq!(get_row(&conn, &id).unwrap().category_id, "read");
        assert_eq!(rules_examples(&conn), 1);

        assert_eq!(undo_event(&mut conn, &events[0]).unwrap(), id);
        let row = get_row(&conn, &id).unwrap();
        assert_eq!(row.category_id, "unsorted");
        assert_eq!(row.triage_source.as_deref(), Some("none"));
        assert_eq!(row.triage_reason.as_deref(), Some("none:no-model"));
        assert_eq!(rules_examples(&conn), 0);
        assert!(matches!(
            undo_event(&mut conn, &events[0]),
            Err(AppError::Code("alreadyUndone"))
        ));
    }

    #[test]
    fn sensitive_recategorize_teaches_no_example() {
        let db = db();
        let mut conn = db.conn();
        let id = insert_item(&mut conn, &new_text("Secret", "", "h1")).unwrap();
        update_triage(&mut conn, &id, &sensitive_update("unsorted")).unwrap();
        recategorize(&mut conn, &[id], "read", &Actor::User).unwrap();
        assert_eq!(rules_examples(&conn), 0);
        assert!(crate::db::rules::recent_examples(&conn, 8).unwrap().is_empty());
    }

    fn rules_examples(conn: &Connection) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM category_examples WHERE deleted_at IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn undo_trash_restores_item() {
        let db = db();
        let mut conn = db.conn();
        let id = insert_item(&mut conn, &new_text("Note", "", "h1")).unwrap();
        let ev = trash(&mut conn, &[id.clone()], &Actor::User).unwrap();
        assert_eq!(counts(&conn).unwrap().inbox, 0);
        undo_event(&mut conn, &ev[0]).unwrap();
        let row = get_row(&conn, &id).unwrap();
        assert_eq!((row.status.as_str(), row.deleted_at), ("inbox", None));
    }

    #[test]
    fn find_live_by_hash_ignores_trashed() {
        let db = db();
        let mut conn = db.conn();
        let id = insert_item(&mut conn, &new_text("A", "", "same")).unwrap();
        assert_eq!(find_live_by_hash(&conn, "same").unwrap(), Some(id.clone()));
        trash(&mut conn, &[id], &Actor::User).unwrap();
        assert_eq!(find_live_by_hash(&conn, "same").unwrap(), None);
    }

    #[test]
    fn list_pages_by_keyset() {
        let db = db();
        let mut conn = db.conn();
        let ids: Vec<String> = (0..5)
            .map(|i| {
                insert_item(&mut conn, &new_text(&format!("t{i}"), "", &format!("h{i}"))).unwrap()
            })
            .collect();
        let paths = Paths::new("/tmp/copet-test".into());
        let mut q = query("");
        q.limit = 2;
        let mut seen = Vec::new();
        loop {
            let page = list_items(&conn, &q, &paths).unwrap();
            seen.extend(page.items.into_iter().map(|i| i.id));
            match page.next_cursor {
                Some(c) => q.cursor = Some(c),
                None => break,
            }
        }
        // UUIDv7 ids created in one ms still sort by creation, so newest-first is reversed.
        assert_eq!(seen, ids.into_iter().rev().collect::<Vec<_>>());
    }

    #[test]
    fn summary_carries_task_suggestion_and_added_by() {
        let db = db();
        let mut conn = db.conn();
        let id = insert_item(&mut conn, &new_text("Deck", "send the deck", "h1")).unwrap();
        let mut u = sensitive_update("todo");
        u.sensitive = false;
        u.task_suggestion = Some("Send the deck".into());
        update_triage(&mut conn, &id, &u).unwrap();
        conn.execute("UPDATE items SET added_by = 'client-1' WHERE id = ?1", [&id]).unwrap();
        let paths = Paths::new("/tmp/copet-test".into());
        let listed = list_items(&conn, &query(""), &paths).unwrap().items.remove(0);
        let detail = get_item_detail(&conn, &id, &paths).unwrap().item;
        for s in [listed, detail] {
            assert_eq!(s.task_suggestion.as_deref(), Some("Send the deck"));
            assert_eq!(s.added_by.as_deref(), Some("client-1"));
        }
    }

    #[test]
    fn summary_flags_too_large_file() {
        let db = db();
        let mut conn = db.conn();
        let id = insert_item(&mut conn, &new_text("Clip", "", "h1")).unwrap();
        conn.execute("UPDATE items SET file_path = 'library/ab/clip.mov' WHERE id = ?1", [&id]).unwrap();
        let paths = Paths::new("/tmp/copet-test".into());
        let flag = |conn: &Connection| {
            let listed = list_items(conn, &query(""), &paths).unwrap().items.remove(0);
            let detail = get_item_detail(conn, &id, &paths).unwrap().item;
            (listed.file_too_large, detail.file_too_large)
        };
        assert_eq!(flag(&conn), (false, false));
        conn.execute("INSERT INTO sync_blobs (rel, state, updated_at) VALUES ('library/ab/clip.mov', 'up', 1)", [])
            .unwrap();
        assert_eq!(flag(&conn), (false, false));
        conn.execute("UPDATE sync_blobs SET state = 'tooLarge'", []).unwrap();
        assert_eq!(flag(&conn), (true, true));
    }

    fn triaged(conn: &mut Connection, title: &str, hash: &str) -> String {
        let id = insert_item(conn, &new_text(title, "", hash)).unwrap();
        let mut u = sensitive_update("read");
        u.sensitive = false;
        update_triage(conn, &id, &u).unwrap();
        id
    }

    #[test]
    fn agent_filter_applies_before_limit() {
        let db = db();
        let mut conn = db.conn();
        let visible = triaged(&mut conn, "Old note", "h1");
        for (title, hash) in [("Secret one", "h2"), ("Secret two", "h3")] {
            let id = insert_item(&mut conn, &new_text(title, "", hash)).unwrap();
            update_triage(&mut conn, &id, &sensitive_update("read")).unwrap();
        }
        let paths = Paths::new("/tmp/copet-test".into());
        let mut q = query("");
        q.limit = 1;
        assert_ne!(list_items(&conn, &q, &paths).unwrap().items[0].id, visible);
        q.agent_visible = true;
        let page = list_items(&conn, &q, &paths).unwrap();
        assert_eq!(page.items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), [visible.as_str()]);
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn untriaged_item_hidden_from_agents() {
        let db = db();
        let mut conn = db.conn();
        let id = insert_item(&mut conn, &new_text("Fresh", "", "h1")).unwrap();
        assert!(!agent_visible(&conn, &id).unwrap());
        let mut u = sensitive_update("read");
        u.sensitive = false;
        update_triage(&mut conn, &id, &u).unwrap();
        assert!(agent_visible(&conn, &id).unwrap());
        conn.execute("UPDATE categories SET exclude_from_mcp = 1 WHERE id = 'read'", []).unwrap();
        assert!(!agent_visible(&conn, &id).unwrap());
    }

    #[test]
    fn agent_tags_are_attributed_and_undoable() {
        let db = db();
        let mut conn = db.conn();
        let id = triaged(&mut conn, "Note", "h1");
        let agent = Actor::Agent("client-1".into());
        let ev = set_tags(&mut conn, &id, &["a".into(), "b".into()], &agent).unwrap();
        assert_eq!(get_row(&conn, &id).unwrap().tags, ["a", "b"]);
        let who: (String, Option<String>) = conn
            .query_row("SELECT actor, client_id FROM item_events WHERE id = ?1", [&ev], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(who, ("agent".into(), Some("client-1".into())));
        undo_event(&mut conn, &ev).unwrap();
        assert!(get_row(&conn, &id).unwrap().tags.is_empty());
        assert!(matches!(archive(&mut conn, &[id], &Actor::Next), Err(AppError::Code("invalid"))));
    }

    #[test]
    fn agent_recategorize_marks_reason_agent() {
        let db = db();
        let mut conn = db.conn();
        let id = triaged(&mut conn, "Note", "h1");
        let ev = recategorize(&mut conn, &[id.clone()], "read", &Actor::Agent("client-1".into())).unwrap();
        let row = get_row(&conn, &id).unwrap();
        assert_eq!((row.category_id.as_str(), row.triage_reason.as_deref()), ("read", Some("agent")));
        let detail = get_item_detail(&conn, &id, &Paths::new("/tmp/copet-test".into())).unwrap();
        let event = detail.events.iter().find(|e| e.id == ev[0]).unwrap();
        assert_eq!(event.client_id.as_deref(), Some("client-1"));
    }
}
