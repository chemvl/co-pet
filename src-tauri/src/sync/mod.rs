//! Encrypted sync engine. Local change tracking lives in schema V8 (`db/migrations.rs`):
//! triggers on every table in `SYNC_TABLES` queue writes in `sync_outbox`. Every synced row is
//! one server `records` row holding `base64(seal(record_key, "<tbl>\0<id>", json))` (`codec`).
//! A run (`engine`) pushes the outbox, pulls by server `seq` and applies the pulled rows
//! (`apply`), then moves library files (`blobs`); `remote` talks to Supabase.
//!
//! `sync_guard` holds exactly one row and is only ever `UPDATE`d (`applying = 1` while `apply`
//! writes pulled rows, so the triggers don't echo them back); never insert or delete its rows.
//! `sync_state` keys: `cursor` (last applied server `seq`), `initializedFor` (the user whose
//! first run queued every row) and `lastSyncedAt` (epoch ms).
mod apply;
mod blobs;
mod codec;
mod engine;
mod remote;

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::AppResult;

pub use engine::{forget, last_synced_at, poke, start, status, sync_now};

/// Synced tables: (table, key column, push order). Push order keeps parents ahead of children.
pub const SYNC_TABLES: [(&str, &str, u8); 9] = [
    ("categories", "id", 0),
    ("settings", "key", 1),
    ("rules", "id", 2),
    ("items", "id", 3),
    ("category_examples", "id", 4),
    ("tasks", "id", 5),
    ("export_targets", "id", 6),
    ("task_exports", "id", 7),
    ("meetings", "id", 8),
];

/// Settings keys that sync across devices; every other key is per-device.
pub const SYNCED_SETTINGS: [&str; 20] = [
    "ui.language",
    "pet.name",
    "pet.style",
    "pet.skin",
    "pet.size",
    "pet.sleepHours",
    "capture.removeOriginal",
    "capture.excludedApps",
    "triage.confidenceThreshold",
    "privacy.secretDetection",
    "whatsNext.onReturn",
    "whatsNext.afterDone",
    "whatsNext.idleMinutes",
    "whatsNext.workHours",
    "whatsNext.maxPerHour",
    "transcribe.language",
    "transcribe.summaryLanguage",
    "transcribe.audioRetentionDays",
    "transcribe.consentShown",
    "whatsNext.afterMeeting",
];

const CURSOR: &str = "cursor";
const INITIALIZED_FOR: &str = "initializedFor";
const LAST_SYNCED_AT: &str = "lastSyncedAt";

/// The synced table named `tbl` and its key column, as the static names SQL may use; `None` for
/// any other name (e.g. a table of a newer client).
fn synced_table(tbl: &str) -> Option<(&'static str, &'static str)> {
    SYNC_TABLES.iter().find(|(t, _, _)| *t == tbl).map(|(t, key, _)| (*t, *key))
}

fn state_get(conn: &Connection, key: &str) -> AppResult<Option<String>> {
    Ok(conn.query_row("SELECT value FROM sync_state WHERE key = ?1", [key], |r| r.get(0)).optional()?)
}

fn state_set(conn: &Connection, key: &str, value: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO sync_state (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Two in-memory "devices", one test master key and the pure sync run, for the `sync/*` tests.
#[cfg(test)]
mod testing {
    use rusqlite::Connection;
    use serde_json::json;

    use super::codec::{read_row, seal_row, Keys};
    use super::engine::{sync_with, Ctx};
    use super::remote::{MemoryRemote, Remote, RemoteRow, SyncError};
    use super::synced_table;
    use crate::db::{items, settings, Db};
    use crate::library;
    use crate::model::NewItem;
    use crate::paths::Paths;

    pub const USER: &str = "user-1";

    pub struct Device {
        pub db: Db,
        pub paths: Paths,
        _dir: tempfile::TempDir,
    }

    /// A fresh database and library with `sync.deviceId = device_id`.
    pub fn device(device_id: &str) -> Device {
        let db = Db::open_in_memory().unwrap();
        settings::put(&db.conn(), "sync.deviceId", &json!(device_id)).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure().unwrap();
        Device { db, paths, _dir: dir }
    }

    pub fn keys() -> Keys {
        Keys::derive(&[7; 32])
    }

    pub async fn sync_device<R: Remote>(d: &Device, remote: &R, keys: &Keys) -> Result<(), SyncError> {
        let cx = Ctx { db: &d.db, paths: &d.paths, keys, user_id: USER };
        sync_with(&cx, remote, &mut |_| {}).await
    }

    /// One sync run that must succeed.
    pub async fn sync(d: &Device, remote: &MemoryRemote, keys: &Keys) {
        sync_device(d, remote, keys).await.unwrap();
    }

    pub fn outbox_len(c: &Connection) -> i64 {
        c.query_row("SELECT count(*) FROM sync_outbox", [], |r| r.get(0)).unwrap()
    }

    /// A file item whose content is `bytes`, stored in `d`'s library; its id and library path.
    pub fn file_item(d: &Device, bytes: &[u8], ext: &str) -> (String, String) {
        let stored = library::store_bytes(&d.paths, bytes, ext).unwrap();
        let new = NewItem {
            kind: "file".into(),
            title: format!("file.{ext}"),
            file_path: Some(stored.rel_path.clone()),
            file_name: Some(format!("file.{ext}")),
            size_bytes: Some(bytes.len() as i64),
            source_kind: "drop".into(),
            content_hash: stored.sha256,
            ..Default::default()
        };
        (items::insert_item(&mut d.db.conn(), &new).unwrap(), stored.rel_path)
    }

    /// The server row for `conn`'s row `tbl`/`id`, as `device_id` would push it.
    pub fn sealed(conn: &Connection, keys: &Keys, tbl: &str, id: &str, device_id: &str, seq: i64) -> RemoteRow {
        let (tbl, key) = synced_table(tbl).unwrap();
        let row = read_row(conn, tbl, key, id).unwrap().unwrap();
        RemoteRow {
            tbl: tbl.into(),
            id: id.into(),
            updated_at: row["updated_at"].as_i64().unwrap(),
            device_id: device_id.into(),
            deleted: false,
            payload: seal_row(&keys.record, tbl, id, &row),
            seq,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn outbox(c: &Connection) -> Vec<(String, String, i64)> {
        let mut st = c.prepare("SELECT tbl, id, queued_at FROM sync_outbox ORDER BY tbl, id").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect()
    }

    /// The quoted keys of the settings triggers' `NEW.key IN (…)` list.
    fn settings_in_list(sql: &str) -> Vec<String> {
        let start = sql.find("NEW.key IN (").expect("settings trigger filters keys") + "NEW.key IN (".len();
        let end = start + sql[start..].find(')').unwrap();
        let mut keys: Vec<String> = sql[start..end].split(',').map(|k| k.trim().trim_matches('\'').to_string()).collect();
        keys.sort();
        keys
    }

    #[test]
    fn trigger_lists_match_constants() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn();
        let mut synced: Vec<String> = SYNCED_SETTINGS.iter().map(|k| k.to_string()).collect();
        synced.sort();
        for (tbl, _, _) in SYNC_TABLES {
            for op in ["ins", "upd"] {
                let name = format!("sync_{tbl}_{op}");
                let sql: String = c
                    .query_row("SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = ?1 AND tbl_name = ?2", [&name, tbl], |r| r.get(0))
                    .unwrap_or_else(|_| panic!("missing trigger {name}"));
                if tbl == "settings" {
                    assert_eq!(settings_in_list(&sql), synced, "{name} keys differ from SYNCED_SETTINGS");
                }
            }
        }
        let count: i64 = c
            .query_row("SELECT count(*) FROM sqlite_master WHERE type = 'trigger' AND name LIKE 'sync\\_%' ESCAPE '\\'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2 * SYNC_TABLES.len() as i64);
    }

    #[test]
    fn local_write_enqueues() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn();
        assert!(outbox(&c).is_empty(), "seed rows are written before the triggers exist");
        let before = crate::db::now_ms();
        c.execute("INSERT INTO tasks (id, title, created_at, updated_at) VALUES ('t1', 'x', 1, 1)", []).unwrap();
        c.execute("UPDATE settings SET value = '\"Mochi\"' WHERE key = 'pet.name'", []).unwrap();
        let rows = outbox(&c);
        assert_eq!(rows.iter().map(|r| (r.0.as_str(), r.1.as_str())).collect::<Vec<_>>(), [("settings", "pet.name"), ("tasks", "t1")]);
        assert!(rows.iter().all(|r| r.2 >= before - 1 && r.2 <= crate::db::now_ms() + 1), "queued_at in ms: {rows:?}");
        // Outer UPSERTs (settings::put) override a trigger's OR-clause; the inner upsert must hold.
        crate::db::settings::put(&c, "pet.name", &serde_json::json!("Pip")).unwrap();
        c.execute("UPDATE tasks SET title = 'y' WHERE id = 't1'", []).unwrap();
        let requeued = outbox(&c);
        assert_eq!(requeued.len(), 2, "re-queue replaces the row");
        for (old, new) in rows.iter().zip(&requeued) {
            assert!(new.2 >= old.2, "queued_at moves forward on re-queue: {old:?} -> {new:?}");
        }
    }

    #[test]
    fn guard_suppresses_outbox() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn();
        assert!(outbox(&c).is_empty());
        c.execute("UPDATE sync_guard SET applying = 1", []).unwrap();
        c.execute("INSERT INTO tasks (id, title, created_at, updated_at) VALUES ('t1', 'x', 1, 1)", []).unwrap();
        c.execute("UPDATE settings SET value = '\"Mochi\"' WHERE key = 'pet.name'", []).unwrap();
        assert!(outbox(&c).is_empty());
    }

    #[test]
    fn device_setting_not_enqueued() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn();
        assert!(outbox(&c).is_empty());
        c.execute("INSERT OR REPLACE INTO settings (key, value, updated_at) VALUES ('hotkeys', '{}', 1)", []).unwrap();
        c.execute("UPDATE settings SET value = '{\"a\":1}' WHERE key = 'hotkeys'", []).unwrap();
        assert!(outbox(&c).is_empty());
    }
}
