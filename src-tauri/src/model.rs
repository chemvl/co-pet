//! Shared domain and IPC types. Everything crossing IPC is camelCase; `src/shared/types.ts`
//! mirrors these structs field for field.
use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::error::{AppError, AppResult};

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
    /// `mcp_clients.id` of the agent that added it; `None` = the user.
    pub added_by: Option<String>,
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
    /// `domain | extension | item_kind | source_kind | source_app | regex`
    pub kind: String,
    pub pattern: String,
    pub builtin: bool,
    pub position: i64,
}

/// A rule in the editor with the number of live items of its category it matches.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleView {
    #[serde(flatten)]
    pub rule: Rule,
    pub match_count: i64,
}

/// A live `category_examples` row.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Example {
    pub id: String,
    pub item_id: String,
    pub title: String,
    pub created_at: i64,
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
    /// `false` when the item's library file isn't on this Mac (synced from another Mac, not
    /// downloaded yet or over the size limit).
    pub file_available: bool,
    /// The item's file is over `sync.maxFileMb` and never leaves this Mac (`sync_blobs`
    /// `tooLarge`).
    pub file_too_large: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemEvent {
    pub id: String,
    pub item_id: String,
    pub at: i64,
    pub actor: String,
    pub client_id: Option<String>,
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
    /// Rust-only (agents): keep items agents may see (`items::AGENT_VISIBLE`).
    #[serde(skip)]
    pub agent_visible: bool,
    /// Rust-only (agents): captured at or after this epoch ms.
    #[serde(skip)]
    pub captured_since: Option<i64>,
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

/// Where this Mac stands with the sync master key (see `account::keys::key_state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum KeyState {
    /// Signed out.
    None,
    /// The master key is on this Mac.
    Ready,
    /// No Mac turned sync on yet: show the new recovery key.
    NeedsSetup,
    /// Another Mac turned sync on: ask for its recovery key.
    NeedsRecovery,
    /// The server couldn't be asked.
    Unknown,
}

/// `account_status`. `user_id` stays set after sign-out (this Mac's data belongs to it);
/// `device_name` is `sync.deviceName`, else the Mac's host name. `last_synced_at` comes from
/// the sync engine (M9).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    pub configured: bool,
    pub signed_in: bool,
    pub email: Option<String>,
    pub user_id: Option<String>,
    pub key_state: KeyState,
    pub device_name: String,
    pub last_synced_at: Option<i64>,
}

/// Where the sync engine stands (`sync_status`, event `sync-status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncState {
    /// No Supabase project in this build, or signed out.
    Off,
    /// `privacy.localOnly` is on.
    Paused,
    /// Signed in, but this Mac has no sync key yet.
    NeedsKey,
    /// The session was refused; sign in again.
    SignedOut,
    Syncing,
    Idle,
    Offline,
    Error,
}

/// `pending`: changes waiting in `sync_outbox`; `blobs_pending`: files waiting to upload or
/// download.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub state: SyncState,
    pub last_synced_at: Option<i64>,
    pub pending: i64,
    pub blobs_pending: i64,
}

/// Who made an item or task change; maps to the `actor` / `client_id` columns of
/// `item_events` and `task_events`. Not serialized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Actor {
    User,
    /// The What's Next card's Done / Snooze.
    Next,
    /// An MCP client (`mcp_clients.id`).
    Agent(String),
}

impl Actor {
    /// `item_events.actor`. `invalid` for `Next`, which the column's CHECK rejects.
    pub fn item_actor(&self) -> AppResult<&'static str> {
        match self {
            Actor::User => Ok("user"),
            Actor::Agent(_) => Ok("agent"),
            Actor::Next => Err(AppError::Code("invalid")),
        }
    }

    /// `task_events.actor`.
    pub fn task_actor(&self) -> AppResult<&'static str> {
        Ok(match self {
            Actor::User => "user",
            Actor::Next => "next",
            Actor::Agent(_) => "agent",
        })
    }

    /// The `client_id` column: the agent's client id, else `None`.
    pub fn client_id(&self) -> Option<&str> {
        match self {
            Actor::Agent(id) => Some(id),
            Actor::User | Actor::Next => None,
        }
    }
}

/// `Some(None)` for an explicit `null`, `None` (via `#[serde(default)]`) when the key is absent.
pub(crate) fn double_option<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
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
    /// The most recently updated live export job, if any.
    pub export: Option<TaskExportState>,
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
    /// Rust-only (agents): keep tasks whose source item agents may see.
    #[serde(skip)]
    pub agent_visible: bool,
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

// ---------- M4: export ----------

/// The latest live `task_exports` row of a task. `state`: `queued | exporting | exported |
/// failed`; `error`: `rateLimited | network | unauthorized | targetMissing | invalid |
/// disconnected`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskExportState {
    pub provider: String,
    pub state: String,
    pub error: Option<String>,
    pub external_url: Option<String>,
    pub next_attempt_at: Option<i64>,
}

/// The live `export_targets` row of a provider; `config` is a `NotionConfig` / `AsanaConfig`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportTarget {
    pub id: String,
    pub provider: String,
    pub account_label: String,
    pub target_label: String,
    pub config: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub provider: String,
    pub connected: bool,
    /// `"oauth" | "token"`
    pub method: Option<String>,
    pub account_label: Option<String>,
    pub needs_reconnect: bool,
    pub target: Option<ExportTarget>,
    /// This device's jobs.
    pub queued: i64,
    pub failed: i64,
    pub next_attempt_at: Option<i64>,
    pub last_error: Option<String>,
}

/// A Notion data source or an Asana workspace / project / section.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotionConfig {
    pub data_source_id: String,
    pub mapping: NotionMapping,
}

/// Notion property names; `None` = "Not mapped".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotionMapping {
    pub title: String,
    pub status: Option<NotionStatusMap>,
    pub due: Option<String>,
    pub tags: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotionStatusMap {
    pub prop: String,
    /// `"status" | "select"`
    pub kind: String,
    pub todo: String,
    pub done: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AsanaConfig {
    pub workspace_gid: String,
    pub project_gid: String,
    pub section_gid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotionSchema {
    pub title: String,
    pub props: Vec<NotionProp>,
    pub suggested: NotionMapping,
}

/// `kind` is the Notion property type (`title`, `status`, `select`, `date`, …); `options` are
/// the option names of `status` / `select` / `multi_select` properties.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotionProp {
    pub name: String,
    pub kind: String,
    pub options: Vec<String>,
}

// ---------- M5: agents ----------

/// `mcp_status`. `http_url` is `http://127.0.0.1:<mcp.httpPort>/mcp`; `stdio_command` is the
/// bundled `copet-mcp`'s absolute path followed by ` --stdio`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    pub paused: bool,
    pub http_state: crate::agent::http::HttpState,
    pub http_url: String,
    pub stdio_command: String,
}

/// An MCP client as the Agents screen lists it. The four known clients are listed before their
/// first contact with `id: None`; `config_installed` says whether their config file has the
/// `copet` entry (always false for other and HTTP clients).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpClientView {
    pub id: Option<String>,
    pub key: String,
    pub name: String,
    pub write_approved: bool,
    pub has_token: bool,
    pub config_installed: bool,
    pub created_at: Option<i64>,
    pub approved_at: Option<i64>,
    pub last_call_at: Option<i64>,
}

/// A bearer token, returned once; only its sha256 is stored.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToken {
    pub token: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpClientCreated {
    pub client: McpClientView,
    pub token: String,
}

/// One `agent_activity` row. `undoable`: a write other than `export_task` that recorded
/// changes (also when it failed part-way), whether or not it was undone since (`undone_at`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityRow {
    pub id: String,
    pub at: i64,
    pub client_name: String,
    pub tool: String,
    pub is_write: bool,
    pub detail: Value,
    pub result_url: Option<String>,
    pub error: Option<String>,
    pub undone_at: Option<i64>,
    pub undoable: bool,
}
