//! The MCP tools. [`dispatch`] resolves the client, checks pause and write approval, runs one
//! tool and logs the call. Agents see only [`items::AGENT_VISIBLE`] items and the tasks of such
//! items; hidden ids answer exactly like missing ones. Everything that needs the running app
//! goes through [`Host`], so the tools run on an in-memory database in tests.
use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveTime, SecondsFormat, TimeZone};
use rusqlite::Connection;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::agent::activity::{self, Entry};
use crate::agent::clients::{self, Client, ClientRef};
use crate::capture::{char_prefix, CaptureInput, CaptureSource};
use crate::db::{categories, items, meetings, now_ms, search, settings, tasks, Db};
use crate::error::AppError;
use crate::export::{self, Provider};
use crate::i18n::Lang;
use crate::model::{
    double_option, Actor, CaptureResult, ItemQuery, NewTask, NextCard, TaskPatch, TaskQuery, TaskSummary,
};
use crate::paths::Paths;

/// `get_item` content is cut to this many characters.
const CONTENT_MAX: usize = 100_000;
const TAGS_MAX: usize = 10;
const TAG_MAX_CHARS: usize = 50;
const TITLE_MAX_CHARS: usize = 300;

/// Every tool and whether it writes.
const TOOLS: [(&str, bool); 12] = [
    ("search_items", false),
    ("get_item", false),
    ("list_tasks", false),
    ("whats_next", false),
    ("list_meetings", false),
    ("get_meeting", false),
    ("add_item", true),
    ("update_item", true),
    ("create_task", true),
    ("update_task", true),
    ("complete_task", true),
    ("export_task", true),
];

/// What a tool needs from the running app. The connection is never held across these calls.
pub trait Host {
    fn db(&self) -> &Db;
    fn paths(&self) -> &Paths;
    /// The app language, for category names.
    fn lang(&self) -> Lang;
    /// Stores the input as new items (events, pet reaction and triage included).
    fn capture(&self, input: CaptureInput, source: CaptureSource) -> CaptureResult;
    /// `next::pick(app, "agent")`.
    fn whats_next(&self) -> Result<Option<NextCard>, AppError>;
    fn items_changed(&self, ids: Vec<String>);
    fn tasks_changed(&self, ids: Vec<String>);
    /// A task became done: celebrate and offer the next card.
    fn task_done(&self);
    fn export_queued(&self, task_id: String);
    /// A read succeeded: the pet glances (throttled).
    fn glance(&self);
}

/// An error for the agent: a protocol code and an English sentence.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolError {
    pub code: &'static str,
    pub message: String,
}

impl ToolError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        ToolError { code, message: message.into() }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        ToolError::new("invalid", message)
    }

    pub fn internal() -> Self {
        ToolError::new("internal", "Co-pet hit an internal error. Try again.")
    }

    fn not_found(what: &str) -> Self {
        ToolError::new("notFound", format!("No {what} with that id."))
    }
}

impl From<AppError> for ToolError {
    fn from(e: AppError) -> Self {
        match e {
            AppError::Code("notFound") => ToolError::new("notFound", "Not found."),
            AppError::Code("invalid") => ToolError::invalid("Invalid arguments."),
            AppError::Code("alreadyExported") => ToolError::invalid("This task is already exported there."),
            other => {
                eprintln!("[agent] {other}");
                ToolError::internal()
            }
        }
    }
}

impl From<rusqlite::Error> for ToolError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::from(e).into()
    }
}

type ToolResult = Result<Value, ToolError>;

/// A notification a write asks for; [`dispatch`] sends it after the activity row is stored.
pub enum Effect {
    ItemsChanged(Vec<String>),
    TasksChanged(Vec<String>),
    TaskDone,
    ExportQueued(String),
}

/// Runs one call for `client` and logs it (unless the client can't be identified). Paused →
/// `paused`; unknown tool → `invalid`; a write by a client without approval →
/// `writesNotApproved`; a write whose activity row can't be stored → `internal`. Successful
/// reads make the pet glance.
pub fn dispatch(host: &impl Host, client: &ClientRef, tool: &str, args: Value) -> ToolResult {
    let now = now_ms();
    let (who, paused) = {
        let conn = host.db().conn();
        let who = clients::resolve(&conn, client, now).map_err(|e| match e {
            AppError::Code("unauthorized") => ToolError::new(
                "unauthorized",
                "The HTTP token is missing or doesn't match. Create one in Co-pet › Agents.",
            ),
            AppError::Code("invalid") => ToolError::invalid(format!(
                "client.key must be 1 to {} characters and client.name 1 to {}.",
                clients::KEY_MAX,
                clients::NAME_MAX
            )),
            other => ToolError::from(other),
        })?;
        (who, settings::get::<bool>(&conn, "mcp.paused")?)
    };
    let is_write = TOOLS.iter().find(|(name, _)| *name == tool).map(|(_, w)| *w);
    let mut log = Entry::new(&who.id, tool, is_write.unwrap_or(false));
    let mut effects = Vec::new();
    let mut result = match is_write {
        _ if paused => Err(ToolError::new(
            "paused",
            "Co-pet's MCP server is paused. Resume it in Co-pet › Agents.",
        )),
        None => Err(ToolError::invalid(format!("Unknown tool {tool}."))),
        Some(true) if !who.write_approved => Err(ToolError::new(
            "writesNotApproved",
            format!("Writes are not approved for {} yet. Approve writes in Co-pet › Agents.", who.name),
        )),
        Some(_) => run(host, &who, tool, args, &mut log, &mut effects),
    };
    if let Err(e) = &result {
        log.error = Some(e.code);
    }
    if let Err(e) = activity::insert(&host.db().conn(), &log, now) {
        eprintln!("[agent] activity log failed for {tool}: {e}");
        // A change nobody can see or undo must not look like a success.
        if is_write == Some(true) && result.is_ok() {
            result = Err(ToolError::internal());
        }
    }
    for effect in effects {
        match effect {
            Effect::ItemsChanged(ids) => host.items_changed(ids),
            Effect::TasksChanged(ids) => host.tasks_changed(ids),
            Effect::TaskDone => host.task_done(),
            Effect::ExportQueued(task_id) => host.export_queued(task_id),
        }
    }
    if result.is_ok() && is_write == Some(false) {
        host.glance();
    }
    result
}

fn run(host: &impl Host, who: &Client, tool: &str, args: Value, log: &mut Entry, fx: &mut Vec<Effect>) -> ToolResult {
    match tool {
        "search_items" => search_items(host, parse(args)?, log),
        "get_item" => get_item(host, parse(args)?, log),
        "list_tasks" => list_tasks(host, parse(args)?, log),
        "whats_next" => whats_next(host, parse(args)?, log),
        "list_meetings" => list_meetings(host, parse(args)?, log),
        "get_meeting" => get_meeting(host, parse(args)?, log),
        "add_item" => add_item(host, who, parse(args)?, log),
        "update_item" => update_item(host, who, parse(args)?, log, fx),
        "create_task" => create_task(host, who, parse(args)?, log, fx),
        "update_task" => update_task(host, who, parse(args)?, log, fx),
        "complete_task" => complete_task(host, who, parse(args)?, log, fx),
        "export_task" => export_task(host, parse(args)?, log, fx),
        _ => unreachable!("dispatch checks the tool name"),
    }
}

/// Strict args; a missing/`null` args object counts as `{}`.
fn parse<T: DeserializeOwned>(args: Value) -> Result<T, ToolError> {
    let args = if args.is_null() { json!({}) } else { args };
    serde_json::from_value(args).map_err(|e| ToolError::invalid(format!("Invalid arguments: {e}.")))
}

// ---------- shared ----------

/// ISO 8601 local time with offset, e.g. `2026-10-17T14:05:00+07:00`.
fn iso(ms: i64) -> Value {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map_or(Value::Null, |d| Value::String(d.to_rfc3339_opts(SecondsFormat::Secs, false)))
}

/// A live category agents may see (not hidden from agents).
fn agent_category(conn: &Connection, id: &str) -> Result<bool, ToolError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM categories WHERE id = ?1 AND deleted_at IS NULL AND exclude_from_mcp = 0)",
        [id],
        |r| r.get(0),
    )?)
}

fn visible_item(conn: &Connection, id: &str) -> Result<(), ToolError> {
    if items::agent_visible(conn, id)? {
        Ok(())
    } else {
        Err(ToolError::not_found("item"))
    }
}

fn visible_task(conn: &Connection, id: &str) -> Result<(), ToolError> {
    if tasks::agent_visible(conn, id)? {
        Ok(())
    } else {
        Err(ToolError::not_found("task"))
    }
}

/// `YYYY-MM-DD` → local midnight in ms.
fn parse_since(d: &str) -> Result<i64, ToolError> {
    NaiveDate::parse_from_str(d, "%Y-%m-%d")
        .ok()
        .and_then(|d| Local.from_local_datetime(&d.and_time(NaiveTime::MIN)).earliest())
        .map(|d| d.timestamp_millis())
        .ok_or_else(|| ToolError::invalid("since must look like 2026-10-17."))
}

/// `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM` → normalized `(due_date, due_time)`.
fn parse_due(due: &str) -> Result<(String, Option<String>), ToolError> {
    let bad = || ToolError::invalid("due must look like 2026-10-17 or 2026-10-17T14:30.");
    let (date, time) = match due.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (due, None),
    };
    match tasks::validate_due(Some(date), time).map_err(|_| bad())? {
        (Some(date), time) => Ok((date, time)),
        (None, _) => Err(bad()),
    }
}

fn check_title(title: &str) -> Result<String, ToolError> {
    tasks::valid_title(title).map_err(|_| ToolError::invalid("title must be 1 to 300 characters."))
}

fn check_task_numbers(priority: Option<i64>, estimate_min: Option<i64>) -> Result<(), ToolError> {
    if let Some(p) = priority {
        tasks::validate_priority(p).map_err(|_| ToolError::invalid("priority must be 0 to 3."))?;
    }
    tasks::validate_estimate(estimate_min).map_err(|_| ToolError::invalid("estimateMin must be 1 to 1440."))
}

/// The task as agents see it.
fn task_json(conn: &Connection, t: &TaskSummary) -> Result<Value, ToolError> {
    let notes: String = conn.query_row("SELECT notes FROM tasks WHERE id = ?1", [&t.id], |r| r.get(0))?;
    let due = t.due_date.as_ref().map(|d| match &t.due_time {
        Some(time) => format!("{d}T{time}"),
        None => d.clone(),
    });
    Ok(json!({
        "id": t.id,
        "title": t.title,
        "status": t.status,
        "due": due,
        "priority": t.priority,
        "estimateMin": t.estimate_min,
        "notes": notes,
        "sourceItemId": t.source_item_id,
        "sourceMeetingId": t.source_meeting_id,
        "export": t.export.as_ref().map(|e| json!({
            "provider": e.provider,
            "state": e.state,
            "url": e.external_url,
        })),
    }))
}

// ---------- reads ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SearchArgs {
    query: Option<String>,
    category: Option<String>,
    since: Option<String>,
    limit: Option<i64>,
}

fn search_items(host: &impl Host, a: SearchArgs, log: &mut Entry) -> ToolResult {
    let limit = a.limit.unwrap_or(20);
    if !(1..=50).contains(&limit) {
        return Err(ToolError::invalid("limit must be 1 to 50."));
    }
    let since = a.since.as_deref().map(parse_since).transpose()?;
    let lang = host.lang();
    let conn = host.db().conn();
    if let Some(cat) = &a.category {
        if !agent_category(&conn, cat)? {
            return Err(ToolError::invalid(format!("Unknown category {cat}.")));
        }
    }
    let q = ItemQuery {
        view: "all".into(),
        category_id: a.category,
        search: a.query.clone(),
        sort: None,
        limit,
        cursor: None,
        agent_visible: true,
        captured_since: since,
    };
    let page = items::list_items(&conn, &q, host.paths())?;
    let mut names: HashMap<String, String> = HashMap::new();
    let mut out = Vec::with_capacity(page.items.len());
    for s in page.items {
        if !names.contains_key(&s.category_id) {
            let name = categories::display_name(&conn, &s.category_id, lang)?;
            names.insert(s.category_id.clone(), name);
        }
        out.push(json!({
            "id": s.id,
            "title": s.title,
            "kind": s.kind,
            "category": s.category_id,
            "categoryName": names[&s.category_id],
            "summary": s.summary,
            "url": s.url,
            "tags": s.tags,
            "capturedAt": iso(s.captured_at),
            "addedByAgent": s.added_by.is_some(),
        }));
    }
    let mut detail = Map::new();
    if let Some(query) = a.query {
        detail.insert("query".into(), Value::String(query));
    }
    detail.insert("count".into(), Value::from(out.len()));
    log.detail = Value::Object(detail);
    Ok(Value::Array(out))
}

#[derive(Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Format {
    #[default]
    Markdown,
    Text,
    Path,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetItemArgs {
    id: String,
    #[serde(default)]
    format: Format,
}

fn get_item(host: &impl Host, a: GetItemArgs, log: &mut Entry) -> ToolResult {
    let conn = host.db().conn();
    visible_item(&conn, &a.id)?;
    let row = items::get_row(&conn, &a.id)?;
    let file_path = row.file_path.as_deref().map(|p| host.paths().abs(p).to_string_lossy().into_owned());
    let body = row.payload_text.as_deref().or(row.extracted_text.as_deref());
    let content = match a.format {
        Format::Markdown => {
            let mut md = format!("# {}\n", row.title);
            if let Some(summary) = row.summary.as_deref().filter(|s| !s.trim().is_empty()) {
                md.push('\n');
                md.push_str(summary);
                md.push('\n');
            }
            if let Some(body) = body.filter(|b| !b.trim().is_empty()) {
                md.push('\n');
                md.push_str(body);
            }
            char_prefix(&md, CONTENT_MAX).to_owned()
        }
        Format::Text => char_prefix(body.unwrap_or_default(), CONTENT_MAX).to_owned(),
        Format::Path => file_path
            .clone()
            .ok_or_else(|| ToolError::invalid("format path needs a file item; this item has no file."))?,
    };
    let mut out = Map::new();
    out.insert("id".into(), json!(row.id));
    out.insert("title".into(), json!(row.title));
    out.insert("kind".into(), json!(row.kind));
    out.insert("category".into(), json!(row.category_id));
    out.insert("url".into(), json!(row.url));
    out.insert("capturedAt".into(), iso(row.captured_at));
    out.insert("content".into(), json!(content));
    if let Some(path) = file_path {
        out.insert("filePath".into(), json!(path));
    }
    log.detail = json!({ "id": row.id, "title": row.title });
    Ok(Value::Object(out))
}

#[derive(Deserialize, Default, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum TaskStatus {
    #[default]
    Open,
    Done,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum DueFilter {
    Overdue,
    Today,
    Week,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListTasksArgs {
    #[serde(default)]
    status: TaskStatus,
    due: Option<DueFilter>,
    limit: Option<i64>,
}

/// `overdue`: due before `now` (a date-only task once its day is over); `today`: due today;
/// `week`: due today through this week's Sunday (weeks run Monday to Sunday, as in Home's task
/// groups and What's Next's `dueThisWeek`).
fn due_matches(t: &TaskSummary, filter: DueFilter, now: DateTime<Local>) -> bool {
    let Some(date) = t.due_date.as_deref().and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()) else {
        return false;
    };
    let today = now.date_naive();
    match filter {
        DueFilter::Overdue => match t.due_time.as_deref().and_then(|s| NaiveTime::parse_from_str(s, "%H:%M").ok()) {
            Some(time) => date.and_time(time) < now.naive_local(),
            None => date < today,
        },
        DueFilter::Today => date == today,
        DueFilter::Week => date >= today && date.iso_week() == today.iso_week(),
    }
}

fn list_tasks(host: &impl Host, a: ListTasksArgs, log: &mut Entry) -> ToolResult {
    let limit = a.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err(ToolError::invalid("limit must be 1 to 100."));
    }
    let view = match a.status {
        TaskStatus::Open => "open",
        TaskStatus::Done => "done",
    };
    let conn = host.db().conn();
    let now = Local::now();
    let out = tasks::list(&conn, &TaskQuery { view: view.into(), search: None, agent_visible: true })?
        .iter()
        .filter(|t| a.due.is_none_or(|d| due_matches(t, d, now)))
        .take(limit as usize)
        .map(|t| task_json(&conn, t))
        .collect::<Result<Vec<_>, _>>()?;
    log.detail = json!({ "count": out.len() });
    Ok(Value::Array(out))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

fn whats_next(host: &impl Host, _: NoArgs, log: &mut Entry) -> ToolResult {
    let card = host.whats_next()?;
    log.detail = match &card {
        Some(c) => json!({ "id": c.task_id, "title": c.title }),
        None => json!({ "count": 0 }),
    };
    Ok(serde_json::to_value(card).map_err(AppError::from)?)
}

/// The meetings agents may see: live, done, with an agent-visible item. Columns are qualified
/// with `meetings.`.
fn meeting_visible_sql() -> String {
    format!(
        "meetings.deleted_at IS NULL AND meetings.state = 'done' AND EXISTS \
         (SELECT 1 FROM items WHERE items.meeting_id = meetings.id AND {})",
        items::AGENT_VISIBLE
    )
}

fn visible_meeting(conn: &Connection, id: &str) -> Result<meetings::MeetingRow, ToolError> {
    let visible: bool = conn.query_row(
        &format!("SELECT EXISTS (SELECT 1 FROM meetings WHERE meetings.id = ?1 AND {})", meeting_visible_sql()),
        [id],
        |r| r.get(0),
    )?;
    if !visible {
        return Err(ToolError::not_found("meeting"));
    }
    Ok(meetings::get(conn, id)?)
}

/// Rounded minutes, at least 1.
fn duration_min(ms: i64) -> i64 {
    ((ms + 30_000) / 60_000).max(1)
}

/// The detected language codes, else the chosen ones (none for `auto`).
fn meeting_languages(m: &meetings::MeetingRow) -> Vec<String> {
    if m.langs.is_empty() {
        m.language.split('+').filter(|c| *c != "auto").map(String::from).collect()
    } else {
        m.langs.clone()
    }
}

fn speaker_names(m: &meetings::MeetingRow, lang: Lang) -> Vec<String> {
    m.speakers.iter().map(|s| meetings::display_name(s, lang)).collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListMeetingsArgs {
    query: Option<String>,
    since: Option<String>,
    limit: Option<i64>,
}

fn list_meetings(host: &impl Host, a: ListMeetingsArgs, log: &mut Entry) -> ToolResult {
    let limit = a.limit.unwrap_or(20);
    if !(1..=50).contains(&limit) {
        return Err(ToolError::invalid("limit must be 1 to 50."));
    }
    let since = a.since.as_deref().map(parse_since).transpose()?.unwrap_or(i64::MIN);
    let tokens: Vec<String> =
        a.query.as_deref().map(|q| search::fold(q).split_whitespace().map(String::from).collect()).unwrap_or_default();
    let lang = host.lang();
    let conn = host.db().conn();
    let ids: Vec<String> = conn
        .prepare(&format!(
            "SELECT meetings.id FROM meetings WHERE {} AND meetings.started_at >= ?1 \
             ORDER BY meetings.started_at DESC, meetings.id DESC",
            meeting_visible_sql()
        ))?
        .query_map([since], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut out = Vec::new();
    for id in ids {
        if out.len() as i64 >= limit {
            break;
        }
        let m = meetings::get(&conn, &id)?;
        if !tokens.is_empty() {
            let hay = search::fold(&format!("{}\n{}", m.title, m.summary.join("\n")));
            if !tokens.iter().all(|t| hay.contains(t.as_str())) {
                continue;
            }
        }
        out.push(json!({
            "id": m.id,
            "title": m.title,
            "startedAt": iso(m.started_at),
            "durationMin": duration_min(m.duration_ms),
            "languages": meeting_languages(&m),
            "speakers": speaker_names(&m, lang),
            "summary": m.summary,
            "actionItems": m.actions.len(),
        }));
    }
    let mut detail = Map::new();
    if let Some(query) = a.query {
        detail.insert("query".into(), Value::String(query));
    }
    detail.insert("count".into(), Value::from(out.len()));
    log.detail = Value::Object(detail);
    Ok(Value::Array(out))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "lowercase")]
enum MeetingFormat {
    #[default]
    Markdown,
    Json,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetMeetingArgs {
    id: String,
    #[serde(default)]
    format: MeetingFormat,
    transcript: Option<bool>,
}

fn get_meeting(host: &impl Host, a: GetMeetingArgs, log: &mut Entry) -> ToolResult {
    let lang = host.lang();
    let conn = host.db().conn();
    let m = visible_meeting(&conn, &a.id)?;
    let transcript = a.transcript.unwrap_or(true);
    log.detail = json!({ "id": m.id, "title": m.title });
    Ok(match a.format {
        MeetingFormat::Markdown => {
            Value::String(char_prefix(&meetings::markdown(&m, lang, transcript), CONTENT_MAX).to_owned())
        }
        MeetingFormat::Json => {
            let mut out = json!({
                "id": m.id,
                "title": m.title,
                "startedAt": iso(m.started_at),
                "durationMin": duration_min(m.duration_ms),
                "languages": meeting_languages(&m),
                "speakers": speaker_names(&m, lang),
                "summary": m.summary,
                "decisions": m.decisions,
                "actionItems": m.actions.iter().map(|ai| json!({
                    "text": ai.text,
                    "owner": match &ai.owner_key {
                        Some(key) => Some(meetings::speaker_name(&m, key, lang)),
                        None => ai.owner.clone(),
                    },
                    "due": ai.due,
                    "taskId": ai.task_id,
                })).collect::<Vec<_>>(),
                "questions": m.questions,
            });
            if transcript {
                out["transcript"] = m
                    .transcript
                    .iter()
                    .map(|s| {
                        let secs = s.start_ms.max(0) / 1000;
                        json!({
                            "start": format!("{:02}:{:02}", secs / 60, secs % 60),
                            "speaker": meetings::speaker_name(&m, &s.speaker, lang),
                            "text": s.text,
                        })
                    })
                    .collect();
            }
            out
        }
    })
}

// ---------- writes ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AddItemArgs {
    text: Option<String>,
    url: Option<String>,
    file_path: Option<String>,
    title: Option<String>,
}

fn add_item(host: &impl Host, who: &Client, a: AddItemArgs, log: &mut Entry) -> ToolResult {
    let input = match (a.text, a.url, a.file_path) {
        (Some(plain), None, None) => CaptureInput::Text { plain, html: None },
        (None, Some(url), None) => {
            if url::Url::parse(url.trim()).is_err() {
                return Err(ToolError::invalid("url must be an absolute URL."));
            }
            CaptureInput::Url { url, title: None }
        }
        (None, None, Some(path)) => {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err(ToolError::invalid("filePath must be an absolute path."));
            }
            if !path.is_file() {
                return Err(ToolError::invalid("filePath must be an existing file."));
            }
            CaptureInput::Files { paths: vec![path] }
        }
        _ => return Err(ToolError::invalid("Give exactly one of text, url or filePath.")),
    };
    if a.title.as_deref().is_some_and(|t| t.trim().chars().count() > TITLE_MAX_CHARS) {
        return Err(ToolError::invalid("title must be at most 300 characters."));
    }
    let source = CaptureSource {
        kind: "drop",
        app_name: Some(who.name.clone()),
        bundle_id: None,
        added_by: Some(who.id.clone()),
        title: a.title,
    };
    let result = host.capture(input, source);
    match result.status {
        "saved" => log.added_item_ids = result.item_ids.clone(),
        // An existing item the agent may not see answers like an unknown one.
        "duplicate" => match result.item_ids.first() {
            Some(id) if items::agent_visible(&host.db().conn(), id)? => {}
            _ => return Err(ToolError::new("notFound", "Co-pet couldn't add it.")),
        },
        "skipped" => return Err(ToolError::invalid("Nothing to add: the text or URL is empty.")),
        _ => {
            eprintln!("[agent] add_item failed: {}", result.message.unwrap_or_default());
            return Err(ToolError::new("internal", "Co-pet couldn't store it."));
        }
    }
    let id = result.item_ids.into_iter().next().ok_or_else(ToolError::internal)?;
    let title = items::get_row(&host.db().conn(), &id)?.title;
    log.detail = json!({ "title": title });
    Ok(json!({ "id": id }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateItemArgs {
    id: String,
    category: Option<String>,
    tags: Option<Vec<String>>,
    archive: Option<bool>,
}

fn update_item(host: &impl Host, who: &Client, a: UpdateItemArgs, log: &mut Entry, fx: &mut Vec<Effect>) -> ToolResult {
    if a.category.is_none() && a.tags.is_none() && a.archive.is_none() {
        return Err(ToolError::invalid("Give at least one of category, tags or archive."));
    }
    let tags = a
        .tags
        .map(|tags| {
            let tags: Vec<String> = tags.iter().map(|t| t.trim().to_owned()).collect();
            let ok = tags.len() <= TAGS_MAX
                && tags.iter().all(|t| !t.is_empty() && t.chars().count() <= TAG_MAX_CHARS);
            ok.then_some(tags)
                .ok_or_else(|| ToolError::invalid("tags must be at most 10 tags of 1 to 50 characters."))
        })
        .transpose()?;
    let lang = host.lang();
    let actor = Actor::Agent(who.id.clone());
    let ids = [a.id.clone()];
    let mut conn = host.db().conn();
    visible_item(&conn, &a.id)?;
    let category_name = match &a.category {
        Some(cat) if agent_category(&conn, cat)? => Some(categories::display_name(&conn, cat, lang)?),
        Some(cat) => return Err(ToolError::invalid(format!("Unknown category {cat}."))),
        None => None,
    };
    let row = items::get_row(&conn, &a.id)?;
    // Each step commits on its own; its events are logged at once so a later failure still
    // leaves the earlier steps undoable.
    let events = &mut log.item_event_ids;
    let steps = (|| -> Result<(), ToolError> {
        if let Some(cat) = &a.category {
            events.extend(items::recategorize(&mut conn, &ids, cat, &actor)?);
        }
        if let Some(tags) = &tags {
            events.push(items::set_tags(&mut conn, &a.id, tags, &actor)?);
        }
        match a.archive {
            Some(true) if row.status != "archived" => events.extend(items::archive(&mut conn, &ids, &actor)?),
            Some(false) if row.status == "archived" => events.extend(items::unarchive(&mut conn, &ids, &actor)?),
            _ => {}
        }
        Ok(())
    })();
    drop(conn);
    if !log.item_event_ids.is_empty() {
        fx.push(Effect::ItemsChanged(ids.to_vec()));
    }
    steps?;
    let events = log.item_event_ids.clone();
    let mut detail = Map::new();
    detail.insert("title".into(), json!(row.title));
    if let (Some(cat), Some(name)) = (a.category, category_name) {
        detail.insert("category".into(), json!(cat));
        detail.insert("categoryName".into(), json!(name));
    }
    if let Some(tags) = tags {
        detail.insert("tags".into(), json!(tags));
    }
    if let Some(archive) = a.archive {
        detail.insert("archived".into(), json!(archive));
    }
    log.detail = Value::Object(detail);
    Ok(json!({ "eventIds": events }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateTaskArgs {
    title: String,
    notes: Option<String>,
    due: Option<String>,
    estimate_min: Option<i64>,
    priority: Option<i64>,
    source_item_id: Option<String>,
}

fn create_task(host: &impl Host, who: &Client, a: CreateTaskArgs, log: &mut Entry, fx: &mut Vec<Effect>) -> ToolResult {
    let title = check_title(&a.title)?;
    let due = a.due.as_deref().map(parse_due).transpose()?;
    check_task_numbers(a.priority, a.estimate_min)?;
    let (due_date, due_time) = due.map_or((None, None), |(d, t)| (Some(d), t));
    let new = NewTask {
        title,
        notes: a.notes,
        due_date,
        due_time,
        priority: a.priority,
        estimate_min: a.estimate_min,
        first_step: None,
        source_item_id: a.source_item_id,
    };
    let mut conn = host.db().conn();
    if let Some(src) = &new.source_item_id {
        visible_item(&conn, src).map_err(|_| ToolError::new("notFound", "No item with that sourceItemId."))?;
    }
    let (task, event) = tasks::create(&mut conn, &new, &Actor::Agent(who.id.clone()))?;
    let out = task_json(&conn, &task)?;
    drop(conn);
    log.task_event_ids = vec![event];
    log.detail = json!({ "title": task.title });
    fx.push(Effect::TasksChanged(vec![task.id]));
    Ok(out)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UpdateTaskArgs {
    id: String,
    title: Option<String>,
    notes: Option<String>,
    /// `null` clears the due date.
    #[serde(default, deserialize_with = "double_option")]
    due: Option<Option<String>>,
    estimate_min: Option<i64>,
    priority: Option<i64>,
}

fn update_task(host: &impl Host, who: &Client, a: UpdateTaskArgs, log: &mut Entry, fx: &mut Vec<Effect>) -> ToolResult {
    if a.title.is_none() && a.notes.is_none() && a.due.is_none() && a.estimate_min.is_none() && a.priority.is_none() {
        return Err(ToolError::invalid("Give at least one of title, notes, due, estimateMin or priority."));
    }
    let title = a.title.as_deref().map(check_title).transpose()?;
    let due = match &a.due {
        Some(Some(d)) => Some(Some(parse_due(d)?)),
        Some(None) => Some(None),
        None => None,
    };
    check_task_numbers(a.priority, a.estimate_min)?;
    let patch = TaskPatch {
        title,
        notes: a.notes,
        due_date: due.as_ref().map(|d| d.as_ref().map(|(date, _)| date.clone())),
        due_time: due.as_ref().map(|d| d.as_ref().and_then(|(_, time)| time.clone())),
        priority: a.priority,
        estimate_min: a.estimate_min.map(Some),
        first_step: None,
    };
    let mut conn = host.db().conn();
    visible_task(&conn, &a.id)?;
    let events = tasks::update(&mut conn, &a.id, &patch, &Actor::Agent(who.id.clone()))?;
    let task = tasks::summary(&conn, &a.id)?;
    let out = task_json(&conn, &task)?;
    drop(conn);
    log.task_event_ids = events;
    log.detail = json!({ "title": task.title });
    fx.push(Effect::TasksChanged(vec![task.id]));
    Ok(out)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdArgs {
    id: String,
}

fn complete_task(host: &impl Host, who: &Client, a: IdArgs, log: &mut Entry, fx: &mut Vec<Effect>) -> ToolResult {
    let mut conn = host.db().conn();
    visible_task(&conn, &a.id)?;
    let events = tasks::set_done(&mut conn, &a.id, true, &Actor::Agent(who.id.clone()))?;
    let task = tasks::summary(&conn, &a.id)?;
    let out = task_json(&conn, &task)?;
    drop(conn);
    let became_done = !events.is_empty();
    log.task_event_ids = events;
    log.detail = json!({ "title": task.title });
    fx.push(Effect::TasksChanged(vec![task.id]));
    if became_done {
        fx.push(Effect::TaskDone);
    }
    Ok(out)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportArgs {
    id: String,
    provider: String,
}

fn export_task(host: &impl Host, a: ExportArgs, log: &mut Entry, fx: &mut Vec<Effect>) -> ToolResult {
    let p: Provider = a.provider.parse().map_err(|_| ToolError::invalid("provider must be notion or asana."))?;
    let name = match p {
        Provider::Notion => "Notion",
        Provider::Asana => "Asana",
    };
    if export::keychain_token(p)?.is_none() {
        return Err(ToolError::new(
            "notConnected",
            format!("{name} isn't connected. Connect it in Co-pet › Integrations."),
        ));
    }
    let conn = host.db().conn();
    visible_task(&conn, &a.id)?;
    if export::target(&conn, p)?.is_none() {
        return Err(ToolError::new(
            "noTarget",
            format!("Choose where tasks go in {name} first, in Co-pet › Integrations."),
        ));
    }
    let title = tasks::summary(&conn, &a.id)?.title;
    let state = export::enqueue(&conn, &a.id, p, Some(&log.id), now_ms())?;
    drop(conn);
    log.detail = json!({ "title": title, "provider": p.as_str() });
    // After the activity row exists: the worker writes its `result_url`.
    fx.push(Effect::ExportQueued(a.id));
    Ok(json!({ "state": state.state }))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::agent::activity::{undo_activity, Undone};
    use crate::capture;
    use crate::model::{NewItem, TriageUpdate};

    pub(crate) struct TestHost {
        pub db: Db,
        pub paths: Paths,
        /// `agent_activity` row count seen by each `items_changed` / `tasks_changed`.
        pub logged_at_notify: std::cell::RefCell<Vec<i64>>,
    }

    impl TestHost {
        pub fn new() -> Self {
            TestHost {
                db: Db::open_in_memory().unwrap(),
                paths: Paths::new("/tmp/copet-test".into()),
                logged_at_notify: Default::default(),
            }
        }

        fn note_notify(&self) {
            // -1 once a test has dropped the table.
            let n = self.db.conn().query_row("SELECT COUNT(*) FROM agent_activity", [], |r| r.get(0)).unwrap_or(-1);
            self.logged_at_notify.borrow_mut().push(n);
        }
    }

    impl Host for TestHost {
        fn db(&self) -> &Db {
            &self.db
        }
        fn paths(&self) -> &Paths {
            &self.paths
        }
        fn lang(&self) -> Lang {
            Lang::En
        }
        fn capture(&self, input: CaptureInput, source: CaptureSource) -> CaptureResult {
            capture::summarize(capture::store(&self.db, &self.paths, Lang::En, input, &source))
        }
        fn whats_next(&self) -> Result<Option<NextCard>, AppError> {
            Ok(None)
        }
        fn items_changed(&self, _: Vec<String>) {
            self.note_notify();
        }
        fn tasks_changed(&self, _: Vec<String>) {
            self.note_notify();
        }
        fn task_done(&self) {}
        fn export_queued(&self, _: String) {}
        fn glance(&self) {}
    }

    fn client(key: &str) -> ClientRef {
        ClientRef { key: key.into(), name: format!("Client {key}"), token: None }
    }

    fn call(h: &TestHost, key: &str, tool: &str, args: Value) -> ToolResult {
        dispatch(h, &client(key), tool, args)
    }

    fn approve(h: &TestHost, key: &str) {
        h.db.conn().execute("UPDATE mcp_clients SET write_approved = 1 WHERE key = ?1", [key]).unwrap();
    }

    fn last_activity(h: &TestHost) -> (String, Option<String>) {
        h.db.conn()
            .query_row("SELECT id, error FROM agent_activity ORDER BY id DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
    }

    /// A triaged item in `category`.
    fn item(h: &TestHost, title: &str, category: &str, sensitive: bool) -> String {
        let mut conn = h.db.conn();
        let id = items::insert_item(
            &mut conn,
            &NewItem {
                kind: "text".into(),
                title: title.into(),
                payload_text: Some(format!("{title} body")),
                source_kind: "drop".into(),
                content_hash: format!("h-{title}"),
                ..Default::default()
            },
        )
        .unwrap();
        let u = TriageUpdate {
            extract_status: "skipped".into(),
            category_id: category.into(),
            triage_source: "rule".into(),
            triage_reason: "test".into(),
            sensitive,
            ..Default::default()
        };
        items::update_triage(&mut conn, &id, &u).unwrap();
        id
    }

    fn ids(v: &Value) -> Vec<&str> {
        v.as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap()).collect()
    }

    /// A done meeting with a transcript, notes and its Inbox item (category `meeting`).
    fn done_meeting(h: &TestHost, title: &str, sensitive: bool) -> String {
        use crate::model::{Speaker, TranscriptSeg};
        let mut conn = h.db.conn();
        let id = meetings::create(&conn, "recording", "en", "d1", 1_000, title, None).unwrap();
        let seg = TranscriptSeg {
            start_ms: 65_000,
            end_ms: 66_000,
            speaker: "me".into(),
            text: "Let's ship Friday".into(),
            lang: "en".into(),
        };
        let me = Speaker { key: "me".into(), name: None };
        meetings::save_transcript(&conn, &id, &[seg], &[me], &["en".into()], false).unwrap();
        meetings::save_notes(&conn, &id, None, &["Release plan agreed".into()], &[], &[], "en").unwrap();
        meetings::set_state(&conn, &id, "done", None).unwrap();
        let item = items::MeetingItem {
            meeting_id: id.clone(),
            title: title.into(),
            summary: Some("Release plan agreed".into()),
            payload: format!("# {title}"),
            sensitive,
            captured_at: 1_000,
        };
        items::upsert_meeting_item(&mut conn, &item).unwrap();
        id
    }

    #[test]
    fn meeting_in_hidden_category_is_not_found() {
        let h = TestHost::new();
        let id = done_meeting(&h, "Standup", false);
        assert!(call(&h, "cursor", "get_meeting", json!({ "id": id })).is_ok());
        h.db.conn().execute("UPDATE categories SET exclude_from_mcp = 1 WHERE id = 'meeting'", []).unwrap();
        let missing = call(&h, "cursor", "get_meeting", json!({ "id": "no-such-id" })).unwrap_err();
        let err = call(&h, "cursor", "get_meeting", json!({ "id": id })).unwrap_err();
        assert_eq!(err, missing);
        assert_eq!(err.code, "notFound");
        let listed = call(&h, "cursor", "list_meetings", json!({})).unwrap();
        assert!(ids(&listed).is_empty());
    }

    #[test]
    fn sensitive_meeting_not_listed() {
        let h = TestHost::new();
        let plain = done_meeting(&h, "Standup", false);
        let secret = done_meeting(&h, "Salary review", true);
        let listed = call(&h, "cursor", "list_meetings", json!({})).unwrap();
        assert_eq!(ids(&listed), [plain.as_str()]);
        assert_eq!(listed[0]["summary"], json!(["Release plan agreed"]));
        assert_eq!(listed[0]["speakers"], json!(["Me"]));
        let found = call(&h, "cursor", "list_meetings", json!({ "query": "RELEASE standup" })).unwrap();
        assert_eq!(ids(&found), [plain.as_str()]);
        let none = call(&h, "cursor", "list_meetings", json!({ "query": "salary" })).unwrap();
        assert!(ids(&none).is_empty());
        assert_eq!(call(&h, "cursor", "get_meeting", json!({ "id": secret })).unwrap_err().code, "notFound");
    }

    #[test]
    fn get_meeting_markdown_has_transcript_by_default() {
        let h = TestHost::new();
        let id = done_meeting(&h, "Standup", false);
        let md = call(&h, "cursor", "get_meeting", json!({ "id": id })).unwrap();
        assert!(md.as_str().unwrap().contains("[01:05] Me: Let's ship Friday"), "{md}");
        let short = call(&h, "cursor", "get_meeting", json!({ "id": id, "transcript": false })).unwrap();
        assert!(!short.as_str().unwrap().contains("Let's ship Friday"));
        let js = call(&h, "cursor", "get_meeting", json!({ "id": id, "format": "json" })).unwrap();
        assert_eq!(js["transcript"], json!([{ "start": "01:05", "speaker": "Me", "text": "Let's ship Friday" }]));
        assert_eq!(call(&h, "cursor", "get_meeting", json!({ "id": id, "format": "html" })).unwrap_err().code, "invalid");
    }

    #[test]
    fn hidden_category_item_is_not_found() {
        let h = TestHost::new();
        let shown = item(&h, "Figma board", "read", false);
        let hidden = item(&h, "Figma secret board", "design", false);
        h.db.conn().execute("UPDATE categories SET exclude_from_mcp = 1 WHERE id = 'design'", []).unwrap();
        let missing = call(&h, "cursor", "get_item", json!({ "id": "no-such-id" })).unwrap_err();
        let err = call(&h, "cursor", "get_item", json!({ "id": hidden })).unwrap_err();
        assert_eq!(err, missing);
        assert_eq!(err.code, "notFound");
        let found = call(&h, "cursor", "search_items", json!({ "query": "figma" })).unwrap();
        assert_eq!(ids(&found), [shown.as_str()]);
        let err = call(&h, "cursor", "search_items", json!({ "category": "design" })).unwrap_err();
        assert_eq!(err.code, "invalid");
        let got = call(&h, "cursor", "get_item", json!({ "id": shown })).unwrap();
        assert_eq!(got["content"], "# Figma board\n\nFigma board body");
    }

    #[test]
    fn sensitive_item_not_searchable() {
        let h = TestHost::new();
        let plain = item(&h, "Deploy notes", "read", false);
        let secret = item(&h, "Deploy token", "read", true);
        let found = call(&h, "cursor", "search_items", json!({ "query": "deploy" })).unwrap();
        assert_eq!(ids(&found), [plain.as_str()]);
        let listed = call(&h, "cursor", "search_items", json!({ "limit": 1 })).unwrap();
        assert_eq!(ids(&listed), [plain.as_str()]);
        assert_eq!(call(&h, "cursor", "get_item", json!({ "id": secret })).unwrap_err().code, "notFound");
        assert_eq!(found[0]["categoryName"], "Read later");
        assert_eq!(found[0]["addedByAgent"], false);
    }

    #[test]
    fn unapproved_client_cannot_write() {
        let h = TestHost::new();
        let err = call(&h, "codex", "create_task", json!({ "title": "Review PR 212" })).unwrap_err();
        assert_eq!(err.code, "writesNotApproved");
        assert!(err.message.contains("Client codex"));
        assert_eq!(last_activity(&h).1.as_deref(), Some("writesNotApproved"));
        assert!(tasks::list(&h.db.conn(), &TaskQuery { view: "open".into(), search: None, agent_visible: false })
            .unwrap()
            .is_empty());
        // Reads stay allowed.
        assert_eq!(call(&h, "codex", "list_tasks", json!({})).unwrap(), json!([]));
        approve(&h, "codex");
        let task = call(&h, "codex", "create_task", json!({ "title": "Review PR 212", "due": "2026-10-20T09:30" })).unwrap();
        assert_eq!((task["title"].as_str(), task["due"].as_str()), (Some("Review PR 212"), Some("2026-10-20T09:30")));
    }

    #[test]
    fn http_token_must_match() {
        let h = TestHost::new();
        h.db.conn()
            .execute(
                "INSERT INTO mcp_clients (id, key, name, write_approved, token_hash, created_at, updated_at)
                 VALUES ('c1', 'http:script', 'Script', 1, ?1, 1, 1)",
                [clients::hash_token("s3cret")],
            )
            .unwrap();
        let with = |token: Option<&str>| ClientRef { key: "http:script".into(), name: "Script".into(), token: token.map(Into::into) };
        let err = dispatch(&h, &with(Some("wrong")), "list_tasks", json!({})).unwrap_err();
        assert_eq!(err.code, "unauthorized");
        assert_eq!(dispatch(&h, &with(None), "list_tasks", json!({})).unwrap_err().code, "unauthorized");
        // Unidentified calls have no client to log against.
        let logged: i64 = h.db.conn().query_row("SELECT COUNT(*) FROM agent_activity", [], |r| r.get(0)).unwrap();
        assert_eq!(logged, 0);
        let ok = ClientRef { key: "anything".into(), name: "Other".into(), token: Some("s3cret".into()) };
        dispatch(&h, &ok, "create_task", json!({ "title": "From script" })).unwrap();
        let (client, last_call): (String, Option<i64>) = h
            .db
            .conn()
            .query_row(
                "SELECT a.client_id, c.last_call_at FROM agent_activity a JOIN mcp_clients c ON c.id = a.client_id",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(client, "c1");
        assert!(last_call.is_some());
    }

    #[test]
    fn paused_rejects_calls() {
        let h = TestHost::new();
        settings::put(&h.db.conn(), "mcp.paused", &json!(true)).unwrap();
        let err = call(&h, "cursor", "search_items", json!({})).unwrap_err();
        assert_eq!(err.code, "paused");
        assert_eq!(err.message, "Co-pet's MCP server is paused. Resume it in Co-pet › Agents.");
        assert_eq!(last_activity(&h).1.as_deref(), Some("paused"));
        settings::put(&h.db.conn(), "mcp.paused", &json!(false)).unwrap();
        assert!(call(&h, "cursor", "search_items", json!({})).is_ok());
    }

    #[test]
    fn undo_activity_reverts_task_and_trashes_added_item() {
        let h = TestHost::new();
        call(&h, "claude-code", "list_tasks", json!({})).unwrap();
        let read = last_activity(&h).0;
        approve(&h, "claude-code");
        let task = call(&h, "claude-code", "create_task", json!({ "title": "Review PR 212" })).unwrap();
        let created = last_activity(&h).0;
        let added = call(&h, "claude-code", "add_item", json!({ "text": "Notes from the agent" })).unwrap();
        let adding = last_activity(&h).0;
        let task_id = task["id"].as_str().unwrap();
        let item_id = added["id"].as_str().unwrap();

        let mut conn = h.db.conn();
        let undoable = |conn: &Connection| -> Vec<(String, bool)> {
            activity::list(conn, false, 10).unwrap().into_iter().map(|r| (r.id, r.undoable)).collect()
        };
        assert_eq!(undoable(&conn), [(adding.clone(), true), (created.clone(), true), (read.clone(), false)]);
        let writes: Vec<String> = activity::list(&conn, true, 10).unwrap().into_iter().map(|r| r.id).collect();
        assert_eq!(writes, [adding.clone(), created.clone()]);
        assert!(matches!(undo_activity(&mut conn, &read), Err(AppError::Code("notUndoable"))));
        let undone = undo_activity(&mut conn, &created).unwrap();
        assert_eq!(undone, Undone { item_ids: vec![], task_ids: vec![task_id.to_owned()] });
        assert!(matches!(tasks::get(&conn, task_id), Err(AppError::Code("notFound"))));
        let undone = undo_activity(&mut conn, &adding).unwrap();
        assert_eq!(undone, Undone { item_ids: vec![item_id.to_owned()], task_ids: vec![] });
        let row = items::get_row(&conn, item_id).unwrap();
        assert_eq!(row.status, "trashed");
        assert!(row.deleted_at.is_some());
        let rows = activity::list(&conn, false, 10).unwrap();
        assert!(rows[1].undone_at.is_some() && rows[1].undoable, "undone writes keep the flag");
        assert_eq!((rows[0].client_name.as_str(), rows[0].tool.as_str()), ("Client claude-code", "add_item"));
        assert!(matches!(undo_activity(&mut conn, &created), Err(AppError::Code("alreadyUndone"))));
        assert!(matches!(undo_activity(&mut conn, "nope"), Err(AppError::Code("notFound"))));
    }

    #[test]
    fn add_item_sets_added_by() {
        let h = TestHost::new();
        approve_after_first_call(&h);
        let err = call(&h, "cursor", "add_item", json!({ "text": "a", "url": "https://x.y" })).unwrap_err();
        assert_eq!(err.code, "invalid");
        let added = call(&h, "cursor", "add_item", json!({ "text": "Read the RFC", "title": "RFC to read" })).unwrap();
        let id = added["id"].as_str().unwrap();
        let conn = h.db.conn();
        let client: String = conn.query_row("SELECT id FROM mcp_clients WHERE key = 'cursor'", [], |r| r.get(0)).unwrap();
        let row = items::get_row(&conn, id).unwrap();
        assert_eq!(row.added_by.as_deref(), Some(client.as_str()));
        assert_eq!(row.title, "RFC to read");
        assert_eq!(row.source_app.as_deref(), Some("Client cursor"));
        let who: (String, Option<String>) = conn
            .query_row("SELECT actor, client_id FROM item_events WHERE item_id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(who, ("agent".into(), Some(client)));
        let added_ids: String = conn
            .query_row("SELECT added_item_ids FROM agent_activity WHERE tool = 'add_item' AND error IS NULL", [], |r| r.get(0))
            .unwrap();
        assert_eq!(added_ids, format!("[\"{id}\"]"));
    }

    fn approve_after_first_call(h: &TestHost) {
        call(h, "cursor", "whats_next", json!({})).unwrap();
        approve(h, "cursor");
    }

    #[test]
    fn write_without_audit_row_is_internal() {
        let h = TestHost::new();
        approve_after_first_call(&h);
        h.db.conn().execute_batch("DROP TABLE agent_activity").unwrap();
        let err = call(&h, "cursor", "create_task", json!({ "title": "Unlogged" })).unwrap_err();
        assert_eq!(err.code, "internal");
        // Reads still answer.
        assert!(call(&h, "cursor", "list_tasks", json!({})).is_ok());
    }

    #[test]
    fn notifications_follow_the_audit_row() {
        let h = TestHost::new();
        approve_after_first_call(&h);
        let task = call(&h, "cursor", "create_task", json!({ "title": "Ship it" })).unwrap();
        call(&h, "cursor", "complete_task", json!({ "id": task["id"] })).unwrap();
        // Each notification saw its own call's row (whats_next is row 1).
        assert_eq!(*h.logged_at_notify.borrow(), [2, 3]);
    }

    #[test]
    fn partial_update_item_stays_undoable() {
        let h = TestHost::new();
        approve_after_first_call(&h);
        let id = item(&h, "Spec", "read", false);
        h.db.conn()
            .execute_batch(
                "CREATE TRIGGER no_tags BEFORE UPDATE OF tags ON items BEGIN SELECT RAISE(ABORT, 'no tags'); END;",
            )
            .unwrap();
        let err = call(&h, "cursor", "update_item", json!({ "id": id, "category": "code", "tags": ["x"] })).unwrap_err();
        assert_eq!(err.code, "internal");
        assert_eq!(items::get_row(&h.db.conn(), &id).unwrap().category_id, "code");
        let activity = last_activity(&h);
        assert_eq!(activity.1.as_deref(), Some("internal"));
        let partial = activity::list(&h.db.conn(), true, 1).unwrap().remove(0);
        assert_eq!((partial.id.as_str(), partial.undoable), (activity.0.as_str(), true));
        let undone = undo_activity(&mut h.db.conn(), &activity.0).unwrap();
        assert_eq!(undone, Undone { item_ids: vec![id.clone()], task_ids: vec![] });
        assert_eq!(items::get_row(&h.db.conn(), &id).unwrap().category_id, "read");
        // A failed call that changed nothing has nothing to undo.
        call(&h, "cursor", "update_item", json!({ "id": "nope", "archive": true })).unwrap_err();
        let failed = last_activity(&h).0;
        assert!(!activity::list(&h.db.conn(), true, 1).unwrap()[0].undoable);
        assert!(matches!(undo_activity(&mut h.db.conn(), &failed), Err(AppError::Code("notUndoable"))));
    }

    #[test]
    fn duplicate_of_hidden_item_reveals_nothing() {
        let h = TestHost::new();
        approve_after_first_call(&h);
        let add = |text: &str| call(&h, "cursor", "add_item", json!({ "text": text }));
        let triage = |id: &str, sensitive: bool| {
            let u = TriageUpdate {
                extract_status: "skipped".into(),
                category_id: "read".into(),
                triage_source: "rule".into(),
                triage_reason: "test".into(),
                sensitive,
                ..Default::default()
            };
            items::update_triage(&mut h.db.conn(), id, &u).unwrap();
        };
        let shared = add("Shared notes").unwrap()["id"].as_str().unwrap().to_owned();
        let secret = add("Secret notes").unwrap()["id"].as_str().unwrap().to_owned();
        // Still waiting for triage: hidden, so a repeat reveals nothing.
        assert_eq!(add("Shared notes").unwrap_err().code, "notFound");
        triage(&shared, false);
        triage(&secret, true);
        assert_eq!(add("Shared notes").unwrap()["id"], shared.as_str());
        let err = add("Secret notes").unwrap_err();
        assert_eq!((err.code, err.message.as_str()), ("notFound", "Co-pet couldn't add it."));
    }

    #[test]
    fn due_filters_follow_the_monday_week() {
        let task = |date: &str, time: Option<&str>| TaskSummary {
            id: "t".into(),
            title: "T".into(),
            status: "open".into(),
            due_date: Some(date.into()),
            due_time: time.map(Into::into),
            priority: 0,
            estimate_min: None,
            first_step: None,
            source_item_id: None,
            source_title: None,
            source_meeting_id: None,
            category_id: "todo".into(),
            added_by: None,
            snoozed_until: None,
            done_at: None,
            created_at: 0,
            updated_at: 0,
            export: None,
        };
        let week = |now: DateTime<Local>| -> Vec<&str> {
            ["2026-10-11", "2026-10-12", "2026-10-14", "2026-10-15", "2026-10-18", "2026-10-19"]
                .into_iter()
                .filter(|d| due_matches(&task(d, None), DueFilter::Week, now))
                .collect()
        };
        // Wednesday 14 Oct 2026: today through Sunday 18.
        let wed = Local.with_ymd_and_hms(2026, 10, 14, 10, 0, 0).unwrap();
        assert_eq!(week(wed), ["2026-10-14", "2026-10-15", "2026-10-18"]);
        // Sunday 18 Oct: only today; Monday 19 is next week.
        let sun = Local.with_ymd_and_hms(2026, 10, 18, 10, 0, 0).unwrap();
        assert_eq!(week(sun), ["2026-10-18"]);
        assert!(due_matches(&task("2026-10-14", Some("09:00")), DueFilter::Overdue, wed));
        assert!(!due_matches(&task("2026-10-14", None), DueFilter::Overdue, wed));
        assert!(due_matches(&task("2026-10-14", None), DueFilter::Today, wed));
    }
}
