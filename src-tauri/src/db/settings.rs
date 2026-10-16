//! Key/value settings; values are JSON. Defaults are seeded by the migration.
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use tauri::{AppHandle, Manager};

use crate::db::{now_ms, Db};
use crate::error::{AppError, AppResult};
use crate::events::{self, SettingsChanged};

pub fn get_all(conn: &Connection) -> AppResult<Map<String, Value>> {
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut map = Map::new();
    for row in rows {
        let (k, v) = row?;
        map.insert(k, serde_json::from_str(&v)?);
    }
    Ok(map)
}

pub fn get_value(conn: &Connection, key: &str) -> AppResult<Option<Value>> {
    let raw: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
        .optional()?;
    Ok(match raw {
        Some(s) => Some(serde_json::from_str(&s)?),
        None => None,
    })
}

/// Typed read. A missing key is an error: every key has a seeded default.
pub fn get<T: DeserializeOwned>(conn: &Connection, key: &str) -> AppResult<T> {
    let v = get_value(conn, key)?.ok_or_else(|| AppError::msg(format!("missing setting {key}")))?;
    Ok(serde_json::from_value(v)?)
}

/// Writes without emitting (tests, migrations of values).
pub fn put(conn: &Connection, key: &str, value: &Value) -> AppResult<()> {
    conn.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![key, serde_json::to_string(value)?, now_ms()],
    )?;
    Ok(())
}

/// Writes and emits `settings-changed {key}`.
pub fn set(app: &AppHandle, key: &str, value: &Value) -> AppResult<()> {
    put(&app.state::<Db>().conn(), key, value)?;
    events::emit(app, events::SETTINGS_CHANGED, SettingsChanged { key: key.to_string() });
    Ok(())
}

/// Typed read through the managed `Db`.
pub fn read<T: DeserializeOwned>(app: &AppHandle, key: &str) -> AppResult<T> {
    get(&app.state::<Db>().conn(), key)
}
