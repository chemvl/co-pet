//! Every `#[tauri::command]`. Commands only translate IPC to module calls; behaviour lives in
//! the modules. Grouped by the plan step that defines them.
use serde_json::{Map, Value};
use tauri::{AppHandle, Manager, State};

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

// ---------- Step 6: capture ----------

#[tauri::command]
pub async fn capture_drop(app: AppHandle, p: crate::model::DropPayload) -> crate::model::CaptureResult {
    crate::capture::drop::run(&app, p).await
}

#[tauri::command]
pub async fn capture_clipboard(app: AppHandle) -> crate::model::CaptureResult {
    crate::capture::clipboard::run(&app, "clipboard").await
}

#[tauri::command]
pub async fn capture_screenshot(app: AppHandle) -> crate::model::CaptureResult {
    crate::capture::screenshot::run(&app).await
}

#[tauri::command]
pub async fn capture_selection(app: AppHandle) -> crate::model::CaptureResult {
    crate::capture::selection::run(&app).await
}

#[tauri::command]
pub async fn capture_note(app: AppHandle, text: String) -> crate::model::CaptureResult {
    use crate::capture::{capture_blocking, CaptureInput, CaptureSource};
    capture_blocking(&app, CaptureInput::Note { text }, CaptureSource::new("note", None)).await
}

// ---------- Step 7: drag-out, copy, paste ----------

#[tauri::command]
pub async fn prepare_drag(app: AppHandle, ids: Vec<String>, as_files: bool) -> AppResult<crate::model::DragPlan> {
    crate::drag::prepare_drag(&app, ids, as_files).await
}

#[tauri::command]
pub async fn copy_items(app: AppHandle, ids: Vec<String>) -> AppResult<()> {
    crate::drag::copy_items(&app, ids).await
}

#[tauri::command]
pub async fn paste_items(app: AppHandle, ids: Vec<String>) -> AppResult<&'static str> {
    crate::drag::paste_items(&app, ids).await
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

// ---------- Step 9: triage + local AI ----------

#[tauri::command]
pub fn ai_status(app: AppHandle, dl: State<'_, crate::llm::download::Downloader>) -> crate::model::AiStatus {
    dl.status(&app)
}

#[tauri::command]
pub fn ai_download(
    app: AppHandle,
    dl: State<'_, crate::llm::download::Downloader>,
    model_id: String,
) -> AppResult<()> {
    dl.start(&app, &model_id)
}

#[tauri::command]
pub fn ai_pause(app: AppHandle, dl: State<'_, crate::llm::download::Downloader>) {
    dl.pause(&app);
}

#[tauri::command]
pub async fn ai_select_model(app: AppHandle, model_id: String) -> AppResult<()> {
    crate::llm::select_model(&app, &model_id).await
}

#[tauri::command]
pub fn recategorize(
    app: AppHandle,
    db: State<'_, Db>,
    ids: Vec<String>,
    category_id: String,
) -> AppResult<crate::model::EventIds> {
    let event_ids = crate::db::items::recategorize(&mut db.conn(), &ids, &category_id, "user")?;
    crate::db::items::notify_changed(&app, ids);
    Ok(crate::model::EventIds { event_ids })
}

#[tauri::command]
pub fn make_rule_from_item(db: State<'_, Db>, item_id: String) -> AppResult<crate::model::Rule> {
    crate::triage::make_rule_from_item(&mut db.conn(), &item_id)
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

// ---------- Step 10: system integration ----------

/// Rejects with `invalid` or `inUse`.
#[tauri::command]
pub fn set_hotkey(app: AppHandle, action: String, accelerator: String) -> AppResult<()> {
    crate::hotkeys::set_hotkey(&app, &action, &accelerator)
}

#[tauri::command]
pub async fn quit_app(app: AppHandle) {
    crate::system::quit(&app).await;
}

#[tauri::command]
pub fn pet_status(app: AppHandle) -> crate::model::PetStatus {
    let visibility = app.state::<crate::platform::macos::fullscreen::Visibility>().current();
    crate::model::PetStatus {
        hidden: visibility.hidden,
        reason: visibility.reason,
        sleep: app.state::<crate::platform::macos::idle::Sleep>().current(),
    }
}

// ---------- Step 11: Home ----------

#[tauri::command]
pub fn open_home(app: AppHandle, route: Option<String>) -> AppResult<()> {
    crate::windows::open_home(&app, route)
}

// ---------- Step 12: Settings ----------

#[tauri::command]
pub async fn pick_excluded_app(app: AppHandle) -> AppResult<Option<String>> {
    crate::data::pick_excluded_app(&app).await
}

#[tauri::command]
pub fn list_excluded_apps(app: AppHandle) -> AppResult<Vec<crate::model::ExcludedApp>> {
    crate::data::list_excluded_apps(&app)
}

#[tauri::command]
pub async fn export_all(app: AppHandle, dir: String) -> AppResult<String> {
    tauri::async_runtime::spawn_blocking(move || crate::data::export_all(&app, std::path::Path::new(&dir)))
        .await
        .map_err(|e| crate::error::AppError::msg(e.to_string()))?
        .map(|out| out.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn delete_everything(app: AppHandle, confirmation: String) -> AppResult<()> {
    crate::data::delete_everything(&app, &confirmation).await
}

// ---------- Step 13: Onboarding ----------

/// Marks onboarding done, shows the pet, applies the login item and closes onboarding.
#[tauri::command]
pub fn finish_onboarding(app: AppHandle) -> AppResult<()> {
    crate::db::settings::set(&app, "onboarding.completed", &Value::Bool(true))?;
    crate::windows::create_pet_window(&app)?;
    crate::system::apply_autostart(&app);
    if let Some(window) = app.get_webview_window(crate::windows::ONBOARDING) {
        window.close()?;
    }
    Ok(())
}
