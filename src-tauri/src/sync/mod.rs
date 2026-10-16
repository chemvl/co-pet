//! Encrypted sync engine. Local change tracking lives in schema V8 (`db/migrations.rs`):
//! triggers on every table in `SYNC_TABLES` queue writes in `sync_outbox`.

/// Synced tables: (table, key column, push order). Push order keeps parents ahead of children.
#[allow(dead_code)] // Task 9.3
pub const SYNC_TABLES: [(&str, &str, u8); 8] = [
    ("categories", "id", 0),
    ("settings", "key", 1),
    ("rules", "id", 2),
    ("items", "id", 3),
    ("category_examples", "id", 4),
    ("tasks", "id", 5),
    ("export_targets", "id", 6),
    ("task_exports", "id", 7),
];

/// Settings keys that sync across devices; every other key is per-device.
#[allow(dead_code)] // Task 9.3
pub const SYNCED_SETTINGS: [&str; 15] = [
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
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use rusqlite::Connection;

    fn outbox(c: &Connection) -> Vec<(String, String, i64)> {
        let mut st = c.prepare("SELECT tbl, id, queued_at FROM sync_outbox ORDER BY tbl, id").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect()
    }

    #[test]
    fn trigger_lists_match_constants() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn();
        for (tbl, _, _) in SYNC_TABLES {
            for op in ["ins", "upd"] {
                let name = format!("sync_{tbl}_{op}");
                let sql: String = c
                    .query_row("SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = ?1 AND tbl_name = ?2", [&name, tbl], |r| r.get(0))
                    .unwrap_or_else(|_| panic!("missing trigger {name}"));
                if tbl == "settings" {
                    for key in SYNCED_SETTINGS {
                        assert!(sql.contains(&format!("'{key}'")), "{name} lacks {key}");
                    }
                }
            }
        }
        let count: i64 = c.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'trigger' AND name LIKE 'sync\\_%' ESCAPE '\\'", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 2 * SYNC_TABLES.len() as i64);
    }

    #[test]
    fn local_write_enqueues() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn();
        c.execute("DELETE FROM sync_outbox", []).unwrap();
        let before = crate::db::now_ms();
        c.execute("INSERT INTO tasks (id, title, created_at, updated_at) VALUES ('t1', 'x', 1, 1)", []).unwrap();
        c.execute("UPDATE settings SET value = '\"Mochi\"' WHERE key = 'pet.name'", []).unwrap();
        let rows = outbox(&c);
        assert_eq!(rows.iter().map(|r| (r.0.as_str(), r.1.as_str())).collect::<Vec<_>>(), [("settings", "pet.name"), ("tasks", "t1")]);
        assert!(rows.iter().all(|r| r.2 >= before - 1 && r.2 <= crate::db::now_ms() + 1), "queued_at in ms: {rows:?}");
        // Outer UPSERTs (settings::put) override a trigger's OR-clause; the inner upsert must hold.
        crate::db::settings::put(&c, "pet.name", &serde_json::json!("Pip")).unwrap();
        c.execute("UPDATE tasks SET title = 'y' WHERE id = 't1'", []).unwrap();
        assert_eq!(outbox(&c).len(), 2, "re-queue replaces the row");
    }

    #[test]
    fn guard_suppresses_outbox() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn();
        c.execute("DELETE FROM sync_outbox", []).unwrap();
        c.execute("UPDATE sync_guard SET applying = 1", []).unwrap();
        c.execute("INSERT INTO tasks (id, title, created_at, updated_at) VALUES ('t1', 'x', 1, 1)", []).unwrap();
        c.execute("UPDATE settings SET value = '\"Mochi\"' WHERE key = 'pet.name'", []).unwrap();
        assert!(outbox(&c).is_empty());
    }

    #[test]
    fn device_setting_not_enqueued() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn();
        c.execute("DELETE FROM sync_outbox", []).unwrap();
        c.execute("INSERT OR REPLACE INTO settings (key, value, updated_at) VALUES ('hotkeys', '{}', 1)", []).unwrap();
        c.execute("UPDATE settings SET value = '{\"a\":1}' WHERE key = 'hotkeys'", []).unwrap();
        assert!(outbox(&c).is_empty());
    }
}
