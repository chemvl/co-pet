//! SQLite access. One connection behind a mutex, managed as Tauri state (`app.state::<Db>()`).
pub mod categories;
pub mod diary;
pub mod items;
pub mod meetings;
pub mod migrations;
pub mod search;
pub mod rules;
pub mod settings;
pub mod tasks;
pub mod undo;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::types::ValueRef;
use rusqlite::{Connection, Row};
use serde_json::{Map, Value};

use crate::error::AppResult;

pub struct Db(Mutex<Connection>);

impl Db {
    /// Opens (or creates) the database and runs migrations.
    pub fn open(path: &Path) -> AppResult<Self> {
        let mut conn = Connection::open(path)?;
        Self::configure(&mut conn)?;
        Ok(Db(Mutex::new(conn)))
    }

    /// In-memory database with migrations applied (tests).
    #[cfg(test)]
    pub fn open_in_memory() -> AppResult<Self> {
        let mut conn = Connection::open_in_memory()?;
        Self::configure(&mut conn)?;
        Ok(Db(Mutex::new(conn)))
    }

    fn configure(conn: &mut Connection) -> AppResult<()> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrations::MIGRATIONS.to_latest(conn)?;
        Ok(())
    }

    pub fn conn(&self) -> MutexGuard<'_, Connection> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Milliseconds since the Unix epoch; every `*_at` column uses this unit.
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// One row as a JSON object keyed by column name (`names`, in column order). Blobs become hex
/// strings.
pub fn row_to_json(row: &Row, names: &[String]) -> AppResult<Map<String, Value>> {
    let mut obj = Map::with_capacity(names.len());
    for (i, name) in names.iter().enumerate() {
        let v = match row.get_ref(i)? {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(n) => Value::from(n),
            ValueRef::Real(f) => Value::from(f),
            ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
            ValueRef::Blob(b) => Value::String(hex::encode(b)),
        };
        obj.insert(name.clone(), v);
    }
    Ok(obj)
}
