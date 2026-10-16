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

#[tauri::command]
pub fn list_items(
    db: State<'_, Db>,
    paths: State<'_, crate::paths::Paths>,
    q: crate::model::ItemQuery,
) -> AppResult<crate::model::ItemPage> {
    crate::db::items::list_items(&db.conn(), &q, &paths)
}

#[tauri::command]
pub fn get_item(
    db: State<'_, Db>,
    paths: State<'_, crate::paths::Paths>,
    id: String,
) -> AppResult<crate::model::ItemDetail> {
    crate::db::items::get_item_detail(&db.conn(), &id, &paths)
}

#[tauri::command]
pub fn counts(db: State<'_, Db>) -> AppResult<crate::model::Counts> {
    crate::db::items::counts(&db.conn())
}

// ---------- Step 4: pet window ----------

#[tauri::command]
pub fn set_hit_regions(
    pointer: State<'_, crate::platform::macos::pointer::PointerState>,
    regions: Vec<crate::model::HitRegion>,
) {
    pointer.set_regions(regions);
}

#[tauri::command]
pub fn set_pet_moving(pointer: State<'_, crate::platform::macos::pointer::PointerState>, moving: bool) {
    pointer.set_moving(moving);
}

#[tauri::command]
pub fn commit_pet_position(app: AppHandle, edge: String, offset: f64) -> AppResult<()> {
    crate::windows::commit_pet_position(&app, &edge, offset)
}

#[tauri::command]
pub fn get_placement(app: AppHandle) -> AppResult<crate::model::Placement> {
    crate::windows::get_placement(&app)
}

#[tauri::command]
pub fn focus_pet_panel(app: AppHandle) -> AppResult<()> {
    crate::windows::focus_pet_panel(&app)
}

#[tauri::command]
pub fn resign_pet_panel(app: AppHandle) -> AppResult<()> {
    crate::windows::resign_pet_panel(&app)
}

// ---------- Step 6: permissions ----------

#[tauri::command]
pub fn request_permission(kind: String) -> AppResult<()> {
    crate::platform::macos::permissions::request(&kind)
}

#[tauri::command]
pub fn permissions_status() -> crate::model::PermissionsStatus {
    crate::platform::macos::permissions::status()
}

// ---------- Step 11: item actions (storage side) ----------

#[tauri::command]
pub fn set_pinned(
    app: AppHandle,
    db: State<'_, Db>,
    id: String,
    pinned: bool,
) -> AppResult<crate::model::EventIds> {
    let event = crate::db::items::set_pinned(&mut db.conn(), &id, pinned)?;
    crate::db::items::notify_changed(&app, vec![id]);
    Ok(crate::model::EventIds { event_ids: vec![event] })
}

#[tauri::command]
pub fn archive(
    app: AppHandle,
    db: State<'_, Db>,
    ids: Vec<String>,
) -> AppResult<crate::model::EventIds> {
    let event_ids = crate::db::items::archive(&mut db.conn(), &ids)?;
    crate::db::items::notify_changed(&app, ids);
    Ok(crate::model::EventIds { event_ids })
}

#[tauri::command]
pub fn trash(
    app: AppHandle,
    db: State<'_, Db>,
    ids: Vec<String>,
) -> AppResult<crate::model::EventIds> {
    let event_ids = crate::db::items::trash(&mut db.conn(), &ids)?;
    crate::db::items::notify_changed(&app, ids);
    Ok(crate::model::EventIds { event_ids })
}

#[tauri::command]
pub fn undo_event(app: AppHandle, db: State<'_, Db>, event_id: String) -> AppResult<()> {
    let item_id = crate::db::items::undo_event(&mut db.conn(), &event_id)?;
    crate::db::items::notify_changed(&app, vec![item_id]);
    Ok(())
}
