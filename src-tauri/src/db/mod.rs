//! SQLite access. One connection behind a mutex, managed as Tauri state (`app.state::<Db>()`).
pub mod items;
pub mod migrations;
pub mod search;
pub mod rules;
pub mod settings;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;

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
