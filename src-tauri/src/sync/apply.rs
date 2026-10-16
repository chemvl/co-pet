//! Applying a page of pulled rows. Foreign keys are off for the page (pages follow server `seq`,
//! not table order, so a child may arrive before its parent) and `sync_guard.applying = 1` keeps
//! the triggers from queueing the writes back into `sync_outbox`.
use std::collections::HashMap;

use rusqlite::types::{ToSql, ToSqlOutput, Value as SqlValue, ValueRef};
use rusqlite::{params, params_from_iter, Connection, ErrorCode, OptionalExtension, Transaction};
use serde_json::{Map, Value};

use super::codec::open_row;
use super::remote::RemoteRow;
use super::{state_set, synced_table, CURSOR, SYNCED_SETTINGS};
use crate::db::{items, now_ms, search};
use crate::error::{AppError, AppResult};
use crate::export;
use crate::paths::Paths;

/// What a page changed, for the events sent after it.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Applied {
    pub item_ids: Vec<String>,
    pub task_ids: Vec<String>,
    /// Categories, rules or category examples changed.
    pub categories: bool,
    pub settings_keys: Vec<String>,
    /// Export targets or export jobs changed.
    pub exports: bool,
}

impl Applied {
    pub fn is_empty(&self) -> bool {
        *self == Applied::default()
    }
}

/// Applies one server page (`rows` in `seq` order) in one transaction, which also stores
/// `cursor` = the last row's `seq`. Rows of unknown tables, rows that don't decrypt, per-device
/// settings and rows this schema rejects (a newer client's value failing a `CHECK` or
/// `NOT NULL`) are skipped. Items are reindexed (FTS) and their files missing here get a `down`
/// row in `sync_blobs`; they are never queued for triage.
pub(crate) fn apply_page(
    conn: &mut Connection,
    rows: &[RemoteRow],
    record_key: &[u8; 32],
    paths: &Paths,
) -> AppResult<Applied> {
    let fk = ForeignKeysOff::new(conn)?;
    let applied = apply_rows(conn, rows, record_key, paths)?;
    fk.restore()?;
    Ok(applied)
}

/// `PRAGMA foreign_keys = OFF` until [`ForeignKeysOff::restore`], or until dropped (an error or
/// a panic), so the shared connection never keeps foreign keys off. Set outside any
/// transaction: inside one the pragma is a no-op.
struct ForeignKeysOff<'c> {
    conn: &'c Connection,
    restored: bool,
}

impl<'c> ForeignKeysOff<'c> {
    fn new(conn: &'c Connection) -> AppResult<Self> {
        conn.pragma_update(None, "foreign_keys", "OFF")?;
        Ok(ForeignKeysOff { conn, restored: false })
    }

    fn restore(mut self) -> AppResult<()> {
        self.restored = true;
        self.conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(())
    }
}

impl Drop for ForeignKeysOff<'_> {
    fn drop(&mut self) {
        if !self.restored {
            if let Err(e) = self.conn.pragma_update(None, "foreign_keys", "ON") {
                eprintln!("[sync] foreign keys back on: {e}");
            }
        }
    }
}

/// Whether `e` is SQLite refusing a value (`CHECK`, `NOT NULL`, …).
fn rejected(e: &AppError) -> bool {
    matches!(e, AppError::Db(rusqlite::Error::SqliteFailure(f, _)) if f.code == ErrorCode::ConstraintViolation)
}

fn apply_rows(conn: &Connection, rows: &[RemoteRow], record_key: &[u8; 32], paths: &Paths) -> AppResult<Applied> {
    // The caller holds the connection exclusively (`&mut` in `apply_page`).
    let tx = conn.unchecked_transaction()?;
    tx.execute("UPDATE sync_guard SET applying = 1", [])?;
    let device_id = export::device_id(&tx)?;
    let now = now_ms();
    let mut columns: HashMap<&str, Vec<String>> = HashMap::new();
    let mut applied = Applied::default();
    for r in rows {
        let Some((tbl, key)) = synced_table(&r.tbl) else { continue };
        let Ok(mut row) = open_row(record_key, &r.tbl, &r.id, &r.payload) else {
            eprintln!("[sync] undecryptable {}/{}", r.tbl, r.id);
            continue;
        };
        if tbl == "settings" && !SYNCED_SETTINGS.contains(&r.id.as_str()) {
            continue;
        }
        if !remote_wins(&tx, tbl, key, r, &device_id)? {
            continue;
        }
        // The sealed place is authoritative for the key.
        row.insert(key.to_string(), Value::String(r.id.clone()));
        if !columns.contains_key(tbl) {
            columns.insert(tbl, table_columns(&tx, tbl)?);
        }
        match upsert(&tx, tbl, key, &columns[tbl], &row) {
            Ok(()) => {}
            // Only this statement is undone; the page goes on.
            Err(e) if rejected(&e) => {
                eprintln!("[sync] rejected {}/{}", r.tbl, r.id);
                continue;
            }
            Err(e) => return Err(e),
        }
        tx.execute("DELETE FROM sync_outbox WHERE tbl = ?1 AND id = ?2", params![tbl, r.id])?;
        match tbl {
            "items" => {
                after_item(&tx, &r.id, paths, now)?;
                applied.item_ids.push(r.id.clone());
            }
            "tasks" => applied.task_ids.push(r.id.clone()),
            "settings" => applied.settings_keys.push(r.id.clone()),
            "export_targets" | "task_exports" => applied.exports = true,
            _ => applied.categories = true,
        }
    }
    if let Some(last) = rows.last() {
        state_set(&tx, CURSOR, &last.seq.to_string())?;
    }
    tx.execute("UPDATE sync_guard SET applying = 0", [])?;
    tx.commit()?;
    Ok(applied)
}

/// A pulled row is the server's winner, so it applies unless this Mac changed the row since
/// (pending in `sync_outbox`) and that change is greater on `(updated_at, device_id)`, the
/// server's own rule. This Mac's own write coming back (same `updated_at` and device) is
/// already here.
fn remote_wins(tx: &Transaction, tbl: &str, key: &str, r: &RemoteRow, device_id: &str) -> AppResult<bool> {
    let local: Option<i64> = tx
        .prepare_cached(&format!("SELECT updated_at FROM {tbl} WHERE {key} = ?1"))?
        .query_row([&r.id], |row| row.get(0))
        .optional()?;
    let Some(local_at) = local else { return Ok(true) };
    if local_at == r.updated_at && r.device_id == device_id {
        return Ok(false);
    }
    let pending: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM sync_outbox WHERE tbl = ?1 AND id = ?2)",
        params![tbl, r.id],
        |row| row.get(0),
    )?;
    Ok(!(pending && (local_at, device_id) > (r.updated_at, r.device_id.as_str())))
}

fn table_columns(tx: &Transaction, tbl: &str) -> AppResult<Vec<String>> {
    let mut stmt = tx.prepare(&format!("PRAGMA table_info({tbl})"))?;
    let columns = stmt.query_map([], |r| r.get::<_, String>(1))?.collect::<rusqlite::Result<_>>()?;
    Ok(columns)
}

/// `INSERT … ON CONFLICT(<key>) DO UPDATE` with the payload's keys that are columns here (a
/// newer client's extra fields are dropped; columns it lacks keep their default or value).
fn upsert(tx: &Transaction, tbl: &str, key: &str, columns: &[String], row: &Map<String, Value>) -> AppResult<()> {
    let cols: Vec<&str> = columns.iter().map(String::as_str).filter(|c| row.contains_key(*c)).collect();
    let marks: Vec<String> = (1..=cols.len()).map(|i| format!("?{i}")).collect();
    let set: Vec<String> = cols.iter().filter(|c| **c != key).map(|c| format!("{c} = excluded.{c}")).collect();
    let on_conflict = if set.is_empty() { "DO NOTHING".to_string() } else { format!("DO UPDATE SET {}", set.join(", ")) };
    let sql = format!(
        "INSERT INTO {tbl} ({}) VALUES ({}) ON CONFLICT({key}) {on_conflict}",
        cols.join(", "),
        marks.join(", ")
    );
    tx.prepare_cached(&sql)?.execute(params_from_iter(cols.iter().map(|c| Param(&row[*c]))))?;
    Ok(())
}

/// A JSON value bound as the SQLite value `db::row_to_json` read it from.
struct Param<'a>(&'a Value);

impl ToSql for Param<'_> {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(match self.0 {
            Value::Null => ToSqlOutput::Borrowed(ValueRef::Null),
            Value::Bool(b) => ToSqlOutput::Owned(SqlValue::Integer(i64::from(*b))),
            Value::Number(n) => ToSqlOutput::Owned(match n.as_i64() {
                Some(i) => SqlValue::Integer(i),
                None => SqlValue::Real(n.as_f64().unwrap_or_default()),
            }),
            Value::String(s) => ToSqlOutput::Borrowed(ValueRef::Text(s.as_bytes())),
            other => ToSqlOutput::Owned(SqlValue::Text(other.to_string())),
        })
    }
}

/// Reindexes the item (no FTS row while deleted) and queues a download for each of its files
/// that isn't on this Mac; a file given up as `missing` is tried afresh.
fn after_item(tx: &Transaction, id: &str, paths: &Paths, now: i64) -> AppResult<()> {
    let item = items::get_row(tx, id)?;
    if item.deleted_at.is_some() {
        search::unindex(tx, id)?;
    } else {
        search::index_item(tx, &item)?;
    }
    for rel in [&item.file_path, &item.thumb_path].into_iter().flatten() {
        if !paths.abs(rel).exists() {
            tx.execute(
                "INSERT INTO sync_blobs (rel, state, updated_at) VALUES (?1, 'down', ?2)
                 ON CONFLICT(rel) DO UPDATE SET state = 'down', attempts = 0, updated_at = excluded.updated_at
                 WHERE state = 'missing'",
                params![rel, now],
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::db::{categories, items as db_items, settings};
    use crate::model::NewItem;
    use crate::sync::testing::{device, keys, outbox_len, sealed};
    use crate::sync::{state_get, SYNC_TABLES};

    fn count(c: &Connection, sql: &str) -> i64 {
        c.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn child_before_parent_applies() {
        let (a, b, keys) = (device("aaaa"), device("bbbb"), keys());
        let (cat, item, example) = {
            let mut c = a.db.conn();
            let cat = categories::create(&mut c, "Receipts", "file", "todo").unwrap().id;
            let new = NewItem {
                kind: "text".into(),
                title: "Lunch receipt".into(),
                payload_text: Some("12.50".into()),
                source_kind: "note".into(),
                content_hash: "h1".into(),
                ..Default::default()
            };
            let item = db_items::insert_item(&mut c, &new).unwrap();
            db_items::recategorize(&mut c, std::slice::from_ref(&item), &cat, &crate::model::Actor::User).unwrap();
            let example: String = c.query_row("SELECT id FROM category_examples", [], |r| r.get(0)).unwrap();
            (cat, item, example)
        };
        // Children first: the example, then its item, then the item's category.
        let page = {
            let c = a.db.conn();
            vec![
                sealed(&c, &keys, "category_examples", &example, "aaaa", 1),
                sealed(&c, &keys, "items", &item, "aaaa", 2),
                sealed(&c, &keys, "categories", &cat, "aaaa", 3),
            ]
        };
        let mut c = b.db.conn();
        let applied = apply_page(&mut c, &page, &keys.record, &b.paths).unwrap();
        assert_eq!(applied.item_ids, [item.clone()]);
        assert!(applied.categories);
        assert_eq!(db_items::get_row(&c, &item).unwrap().category_id, cat);
        assert_eq!(count(&c, "SELECT count(*) FROM category_examples"), 1);
        assert!(categories::exists(&c, &cat).unwrap());
        assert_eq!(count(&c, "PRAGMA foreign_keys"), 1, "foreign keys back on after the page");
        assert_eq!(count(&c, "SELECT count(*) FROM pragma_foreign_key_check"), 0);
        assert_eq!(count(&c, "SELECT applying FROM sync_guard"), 0);
        assert_eq!(outbox_len(&c), 0, "applied rows are not queued");
        assert_eq!(state_get(&c, CURSOR).unwrap().as_deref(), Some("3"));
        let hits = count(&c, "SELECT count(*) FROM items_fts WHERE items_fts MATCH '\"lunch\"*'");
        assert_eq!(hits, 1, "pulled items are searchable");
    }

    #[test]
    fn pending_local_change_wins_only_when_newer() {
        let (a, b, keys) = (device("aaaa"), device("bbbb"), keys());
        let remote_at = |c: &Connection, title: &str, at: i64| {
            c.execute("UPDATE tasks SET title = ?1, updated_at = ?2 WHERE id = 't1'", params![title, at]).unwrap();
        };
        for d in [&a, &b] {
            d.db.conn().execute("INSERT INTO tasks (id, title, created_at, updated_at) VALUES ('t1', 'x', 1, 1)", []).unwrap();
        }
        remote_at(&a.db.conn(), "from A", 200);
        let page = vec![sealed(&a.db.conn(), &keys, "tasks", "t1", "aaaa", 1)];

        // B changed it later: B's pending change stays.
        remote_at(&b.db.conn(), "from B", 300);
        apply_page(&mut b.db.conn(), &page, &keys.record, &b.paths).unwrap();
        let title = |c: &Connection| c.query_row("SELECT title FROM tasks WHERE id = 't1'", [], |r| r.get::<_, String>(0)).unwrap();
        assert_eq!(title(&b.db.conn()), "from B");
        assert_eq!(outbox_len(&b.db.conn()), 1);

        // Same time: the greater device id wins, here B ("bbbb" > "aaaa").
        remote_at(&b.db.conn(), "from B", 200);
        apply_page(&mut b.db.conn(), &page, &keys.record, &b.paths).unwrap();
        assert_eq!(title(&b.db.conn()), "from B");

        // B's pending change is older: the server's row wins and replaces the outbox entry.
        remote_at(&b.db.conn(), "from B", 100);
        apply_page(&mut b.db.conn(), &page, &keys.record, &b.paths).unwrap();
        assert_eq!(title(&b.db.conn()), "from A");
        assert_eq!(outbox_len(&b.db.conn()), 0);
    }

    #[test]
    fn unknown_tables_and_device_settings_are_skipped() {
        let (a, b, keys) = (device("aaaa"), device("bbbb"), keys());
        settings::put(&a.db.conn(), "hotkeys", &json!({"focusPet": "F1"})).unwrap();
        let mut page = vec![sealed(&a.db.conn(), &keys, "settings", "hotkeys", "aaaa", 1)];
        let mut future = page[0].clone();
        future.tbl = "projects".into();
        future.seq = 2;
        page.push(future);
        let applied = apply_page(&mut b.db.conn(), &page, &keys.record, &b.paths).unwrap();
        assert!(applied.is_empty());
        assert_ne!(settings::get::<Value>(&b.db.conn(), "hotkeys").unwrap()["focusPet"], "F1");
        assert_eq!(state_get(&b.db.conn(), CURSOR).unwrap().as_deref(), Some("2"), "skipped rows still advance");
        assert!(SYNC_TABLES.iter().all(|(t, _, _)| *t != "projects"));
    }

    #[test]
    fn rows_this_schema_rejects_are_skipped() {
        let (a, b, keys) = (device("aaaa"), device("bbbb"), keys());
        for id in ["t1", "t2", "t3"] {
            a.db.conn().execute("INSERT INTO tasks (id, title, created_at, updated_at) VALUES (?1, 'ok', 1, 1)", [id]).unwrap();
        }
        let c = a.db.conn();
        // A newer client's status this schema's CHECK refuses, and a NULL title (NOT NULL).
        let with = |id: &str, column: &str, value: Value, seq: i64| {
            let mut r = sealed(&c, &keys, "tasks", id, "aaaa", seq);
            let mut row = crate::sync::codec::open_row(&keys.record, "tasks", id, &r.payload).unwrap();
            row.insert(column.into(), value);
            r.payload = crate::sync::codec::seal_row(&keys.record, "tasks", id, &row);
            r
        };
        let page = vec![with("t1", "status", json!("someday"), 1), with("t2", "title", Value::Null, 2), sealed(&c, &keys, "tasks", "t3", "aaaa", 3)];
        let mut bc = b.db.conn();
        let applied = apply_page(&mut bc, &page, &keys.record, &b.paths).unwrap();
        assert_eq!(applied.task_ids, ["t3"]);
        assert_eq!(count(&bc, "SELECT count(*) FROM tasks"), 1);
        assert_eq!(state_get(&bc, CURSOR).unwrap().as_deref(), Some("3"), "the cursor moves past rejected rows");
        assert_eq!(count(&bc, "SELECT applying FROM sync_guard"), 0);
        assert_eq!(outbox_len(&bc), 0);
    }

    #[test]
    fn foreign_keys_come_back_on_after_a_panic() {
        let b = device("bbbb");
        let c = b.db.conn();
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _off = ForeignKeysOff::new(&c).unwrap();
            assert_eq!(count(&c, "PRAGMA foreign_keys"), 0);
            panic!("mid-page");
        }));
        assert!(panicked.is_err());
        assert_eq!(count(&c, "PRAGMA foreign_keys"), 1);
    }
}
