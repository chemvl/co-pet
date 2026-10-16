//! Every `#[tauri::command]`. Commands only translate IPC to module calls; behaviour lives in
//! the modules. Grouped by the plan step that defines them.
use serde_json::{Map, Value};
use tauri::{AppHandle, State};

use crate::db::{settings, Db};
use crate::error::AppResult;

// ---------- Step 3: storage ----------

#[tauri::command]
pub fn get_settings(db: State<'_, Db>) -> AppResult<Map<String, Value>> {
    settings::get_all(&db.conn())
}

#[tauri::command]
pub fn set_setting(app: AppHandle, key: String, value: Value) -> AppResult<()> {
    settings::set(&app, &key, &value)
}
