//! Shared domain and IPC types. Everything crossing IPC is camelCase; `src/shared/types.ts`
//! mirrors these structs field for field.
use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// Built-in category ids in design-system order; `unsorted` is last. Users add more rows to
/// `categories` (`db::categories`).
pub const BUILTIN_CATEGORY_IDS: [&str; 9] =
    ["todo", "read", "reference", "code", "design", "idea", "contact", "meeting", "unsorted"];

/// Full `items` row. Library paths (`file_path`, `thumb_path`) are relative to `Paths::library`.
#[derive(Debug, Clone, Default)]
pub struct ItemRow {
    pub id: String,
    /// `file | image | url | text | richtext | code | screenshot`
    pub kind: String,
    pub title: String,
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub payload_text: Option<String>,
    pub payload_html: Option<String>,
    pub url: Option<String>,
    pub file_path: Option<String>,
    pub file_name: Option<String>,
    pub mime: Option<String>,
    pub size_bytes: Option<i64>,
    pub thumb_path: Option<String>,
    pub source_app: Option<String>,
    pub source_bundle_id: Option<String>,
    /// `drop | clipboard | screenshot | note | selection`
    pub source_kind: String,
    pub extracted_text: Option<String>,
    /// `pending | done | failed | skipped`
    pub extract_status: String,
    pub category_id: String,
    /// `pending | done`
    pub triage_status: String,
    /// `rule | ai | user | none`
    pub triage_source: Option<String>,
    pub triage_confidence: Option<f64>,
    pub triage_reason: Option<String>,
    pub suggestions: Vec<String>,
    pub sensitive: bool,
    /// `inbox | archived | trashed`
    pub status: String,
    pub pinned: bool,
    pub captured_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
    /// `mcp_clients.id` of the agent that added the item; `None` = the user.
    pub added_by: Option<String>,
    /// AI-suggested task title; `None` for rule-decided or non-actionable items.
    pub task_suggestion: Option<String>,
}

/// Values for a new row; everything else takes the column default.
#[derive(Debug, Clone, Default)]
pub struct NewItem {
    pub kind: String,
    pub title: String,
    pub payload_text: Option<String>,
    pub payload_html: Option<String>,
    pub url: Option<String>,
    pub file_path: Option<String>,
    pub file_name: Option<String>,
    pub mime: Option<String>,
    pub size_bytes: Option<i64>,
    pub thumb_path: Option<String>,
    pub source_app: Option<String>,
    pub source_bundle_id: Option<String>,
    pub source_kind: String,
    pub content_hash: String,
}

/// What the triage worker writes back after extraction + rules/AI.
#[derive(Debug, Clone, Default)]
pub struct TriageUpdate {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub tags: Option<Vec<String>>,
    pub extracted_text: Option<String>,
    pub extract_status: String,
    pub thumb_path: Option<String>,
    pub category_id: String,
    pub triage_source: String,
    pub triage_confidence: Option<f64>,
    pub triage_reason: String,
    pub suggestions: Vec<String>,
    pub sensitive: bool,
    /// AI-suggested task title; `None` for rule-decided or non-actionable items.
    pub task_suggestion: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: String,
    pub category_id: String,
    /// `domain | extension | item_kind | source_kind | regex`
    pub kind: String,
    pub pattern: String,
    pub builtin: bool,
    pub position: i64,
}

/// A live `categories` row. `None` name/icon/color: the built-in i18n name `category.<id>`, the
/// icon named like the id, the colour of the id. `color` is otherwise a built-in id.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: String,
    pub name: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub position: i64,
    pub exclude_from_ai: bool,
    pub exclude_from_mcp: bool,
    pub builtin: bool,
}

/// `update_category`: `None` fields keep their value.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryPatch {
    pub name: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub exclude_from_ai: Option<bool>,
    pub exclude_from_mcp: Option<bool>,
}

// ---------- IPC ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSummary {
    pub id: String,
    pub kind: String,
    pub title: String,
    /// `None` when sensitive.
    pub summary: Option<String>,
    pub tags: Vec<String>,
    pub url: Option<String>,
    pub file_name: Option<String>,
    pub mime: Option<String>,
    pub size_bytes: Option<i64>,
    /// Absolute path; `None` when sensitive.
    pub thumb_path: Option<String>,
    pub source_app: Option<String>,
    pub source_kind: String,
    pub category_id: String,
    pub triage_status: String,
    pub triage_source: Option<String>,
    pub triage_confidence: Option<f64>,
    pub triage_reason: Option<String>,
    pub suggestions: Vec<String>,
    pub sensitive: bool,
    pub status: String,
    pub pinned: bool,
    pub extract_status: String,
    pub captured_at: i64,
    pub updated_at: i64,
    pub added_by: Option<String>,
    pub task_suggestion: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemEvent {
    pub id: String,
    pub item_id: String,
    pub at: i64,
    pub actor: String,
    pub action: String,
    pub before_json: Option<Value>,
    pub after_json: Option<Value>,
    pub undone_at: Option<i64>,
}

/// Payload fields are `None` for sensitive items: the UI never renders them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDetail {
    #[serde(flatten)]
    pub item: ItemSummary,
    pub payload_text: Option<String>,
    pub payload_html: Option<String>,
    pub extracted_text: Option<String>,
    /// Absolute path of the stored blob.
    pub file_path: Option<String>,
    pub events: Vec<ItemEvent>,
    /// Live tasks converted from this item.
    pub tasks: Vec<TaskSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor {
    pub captured_at: i64,
    pub id: String,
}

/// `view`: `all | recent | pinned | unsorted`; `sort`: `newest | oldest`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemQuery {
    pub view: String,
    pub category_id: Option<String>,
    pub search: Option<String>,
    pub sort: Option<String>,
    pub limit: i64,
    pub cursor: Option<Cursor>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemPage {
    pub items: Vec<ItemSummary>,
    pub next_cursor: Option<Cursor>,
}

/// Ids of the `item_events` a write recorded (for Undo).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventIds {
    pub event_ids: Vec<String>,
}

/// `create_task` result: the new task and its `created` event id (for Undo).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCreated {
    pub task: TaskSummary,
    pub event_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub inbox: i64,
    pub unsorted: i64,
    pub pinned: i64,
    pub by_category: BTreeMap<String, i64>,
    pub tasks_open: i64,
    pub tasks_done_today: i64,
    pub task_due_soon: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// A frontend hit region in CSS px relative to the pet window. `kind`: `pet | panel`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HitRegion {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub kind: String,
}

/// `edge`: `left | right | bottom`; `offset` 0..1 along the edge; `work_area` in logical px.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Placement {
    pub monitor_name: String,
    pub edge: String,
    pub offset: f64,
    pub scale: f64,
    pub work_area: Rect,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    pub bytes: u64,
    pub downloaded: bool,
}

/// `state`: `none | downloading | paused | verifying | ready | failed`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    pub model_id: Option<String>,
    pub state: String,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub error: Option<String>,
    /// The registry, for model cards.
    pub models: Vec<ModelInfo>,
    pub recommended_id: String,
}

/// `status`: `saved | duplicate | skipped | failed`. `message` is an i18n key (e.g.
/// `notice.clipboardExcluded`) or, for `failed`, the raw reason; `message_params` fills it.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureResult {
    pub status: &'static str,
    pub item_ids: Vec<String>,
    pub message: Option<String>,
    pub message_params: BTreeMap<String, String>,
}

/// Matches `@crabnebula/tauri-plugin-drag`'s `DragItem`: a list of file paths or typed data.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum DragPlan {
    Files(Vec<String>),
    Data { data: BTreeMap<String, String>, types: Vec<String> },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionsStatus {
    pub screen_recording: bool,
    pub accessibility: bool,
}

/// `list_excluded_apps`: an excluded bundle id and the installed app's name (`None` when the app
/// isn't installed).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcludedApp {
    pub bundle_id: String,
    pub name: Option<String>,
}

/// `pet_status`: the pet's hidden/sleep state now, for a pet window that missed the change events.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PetStatus {
    pub hidden: bool,
    /// `"fullscreen" | "user"` or `None`.
    pub reason: Option<&'static str>,
    pub sleep: bool,
}

/// `backend_status`: whether this build has a Supabase URL and key.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendStatus {
    pub configured: bool,
}

/// Who made a task change; maps to the `task_events.actor` / `client_id` columns. Not serialized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Actor {
    User,
    /// The What's Next card's Done / Snooze.
    Next,
}

impl Actor {
    /// `(actor, client_id)` column values.
    pub fn columns(&self) -> (&'static str, Option<&str>) {
        match self {
            Actor::User => ("user", None),
            Actor::Next => ("next", None),
        }
    }
}

/// `Some(None)` for an explicit `null`, `None` (via `#[serde(default)]`) when the key is absent.
fn double_option<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTask {
    pub title: String,
    pub notes: Option<String>,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub priority: Option<i64>,
    pub estimate_min: Option<i64>,
    pub first_step: Option<String>,
    pub source_item_id: Option<String>,
}

/// Absent field = keep; for `Option<Option<_>>` fields, `null` = clear.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPatch {
    pub title: Option<String>,
    pub notes: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub due_date: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub due_time: Option<Option<String>>,
    pub priority: Option<i64>,
    #[serde(default, deserialize_with = "double_option")]
    pub estimate_min: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub first_step: Option<Option<String>>,
}

/// `source_title` is NULL when there is no source or it is trashed/deleted; `category_id` is the
/// source item's category, else `todo`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSummary {
    pub id: String,
    pub title: String,
    pub status: String,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub priority: i64,
    pub estimate_min: Option<i64>,
    pub first_step: Option<String>,
    pub source_item_id: Option<String>,
    pub source_title: Option<String>,
    pub category_id: String,
    pub added_by: Option<String>,
    pub snoozed_until: Option<i64>,
    pub done_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskEvent {
    pub id: String,
    pub at: i64,
    pub actor: String,
    pub client_id: Option<String>,
    pub action: String,
    pub before_json: Option<String>,
    pub after_json: Option<String>,
    pub undone_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDetail {
    #[serde(flatten)]
    pub task: TaskSummary,
    pub notes: String,
    pub events: Vec<TaskEvent>,
}

/// `view`: `open | done`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskQuery {
    pub view: String,
    pub search: Option<String>,
}

/// The What's Next card. `reason` is the model's sentence; `reason_key` the ranking reason
/// (`overdue | dueSoon | dueToday | dueTomorrow | dueThisWeek | highPriority | waiting`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NextCard {
    pub task_id: String,
    pub title: String,
    pub reason: Option<String>,
    pub reason_key: Option<String>,
    pub first_step: Option<String>,
    pub estimate_min: Option<i64>,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub category_id: String,
    pub source_title: Option<String>,
    /// Task creation time (epoch ms): the `waiting` reason's date.
    pub created_at: i64,
    /// `"ai" | "rank"`
    pub picked_by: String,
}
