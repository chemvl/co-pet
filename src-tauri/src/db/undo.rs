//! Shared Undo step: write an event's `before_json` back onto its row.
use rusqlite::types::Value as SqlValue;
use rusqlite::{params_from_iter, Transaction};
use serde_json::Value;

use crate::error::{AppError, AppResult};

pub(crate) fn json_to_sql(v: &Value) -> SqlValue {
    match v {
        Value::Null => SqlValue::Null,
        Value::Bool(b) => SqlValue::Integer(i64::from(*b)),
        Value::Number(n) => match n.as_i64() {
            Some(i) => SqlValue::Integer(i),
            None => SqlValue::Real(n.as_f64().unwrap_or_default()),
        },
        Value::String(s) => SqlValue::Text(s.clone()),
        other => SqlValue::Text(other.to_string()),
    }
}

/// Sets every `allowed` key of `before` (a JSON object) on row `id_col = id` of `table` and bumps
/// `updated_at` to `now`. `notUndoable` when `before` is not an object or has no allowed key.
/// `table`, `id_col` and `allowed` are code constants, never user input.
pub fn restore_before(
    tx: &Transaction<'_>,
    table: &str,
    id_col: &str,
    id: &str,
    before: Option<&Value>,
    allowed: &[&str],
    now: i64,
) -> AppResult<()> {
    let Some(Value::Object(before)) = before else {
        return Err(AppError::Code("notUndoable"));
    };
    let mut sets = Vec::new();
    let mut values = vec![SqlValue::Text(id.to_owned()), SqlValue::Integer(now)];
    for (key, value) in before {
        if allowed.contains(&key.as_str()) {
            values.push(json_to_sql(value));
            sets.push(format!("{key} = ?{}", values.len()));
        }
    }
    if sets.is_empty() {
        return Err(AppError::Code("notUndoable"));
    }
    tx.execute(
        &format!(
            "UPDATE {table} SET {}, updated_at = ?2 WHERE {id_col} = ?1",
            sets.join(", ")
        ),
        params_from_iter(values),
    )?;
    Ok(())
}
