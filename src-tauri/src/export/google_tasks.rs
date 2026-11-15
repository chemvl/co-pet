//! Google Tasks API. Requests are built and responses parsed by pure functions; the async
//! functions only send them. An exported task is addressed by its list and its id, so its
//! `external_id` is `<tasklist id>:<task id>` (Google ids are base64url, without `:`).
use chrono::NaiveDate;
use reqwest::RequestBuilder;
use serde_json::{json, Map, Value};

use super::{path_segment, rfc3339_ms, send, ExportError, ExportPayload, Exported, RemoteState};
use crate::backend;
use crate::model::{GoogleTasksConfig, TargetOption};

const BASE: &str = "https://tasks.googleapis.com/tasks/v1";
const USERINFO: &str = "https://openidconnect.googleapis.com/v1/userinfo";
/// Where a task without `webViewLink` in the response opens.
const WEB: &str = "https://tasks.google.com/";
/// Google's limits on `title` and `notes`, in characters.
const MAX_TITLE: usize = 1024;
const MAX_NOTES: usize = 8192;

fn get(url: &str, token: &str) -> RequestBuilder {
    backend::http().get(url).bearer_auth(token)
}

fn task_url(list: &str, task: &str) -> String {
    format!("{BASE}/lists/{}/tasks/{}", path_segment(list), path_segment(task))
}

// ---------- pure ----------

/// `email`, else `name`, else "Google".
pub fn parse_me(v: &Value) -> String {
    [&v["email"], &v["name"]]
        .into_iter()
        .filter_map(Value::as_str)
        .find(|s| !s.trim().is_empty())
        .unwrap_or("Google")
        .to_string()
}

/// `items[] {id, title}`.
pub fn parse_lists(v: &Value) -> Vec<TargetOption> {
    v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|o| Some(TargetOption { id: o["id"].as_str()?.into(), label: o["title"].as_str().unwrap_or("").into() }))
        .collect()
}

/// The first `max` characters of `s`.
fn clip(s: &str, max: usize) -> &str {
    s.char_indices().nth(max).map_or(s, |(i, _)| &s[..i])
}

/// The `POST /lists/{list}/tasks` body. The API keeps only the date of `due`, so a due time is
/// dropped; notes are cut so the source line always fits Google's 8192 characters.
pub fn task_body(p: &ExportPayload) -> Value {
    let source = format!("Source: {}", p.source_url);
    let notes = p.notes.trim();
    let notes = if notes.is_empty() {
        source
    } else {
        let room = MAX_NOTES.saturating_sub(source.chars().count() + 2);
        format!("{}\n\n{source}", clip(notes, room))
    };
    let mut data = Map::new();
    data.insert("title".into(), json!(clip(&p.title, MAX_TITLE)));
    data.insert("notes".into(), json!(clip(&notes, MAX_NOTES)));
    data.insert("status".into(), json!(if p.done { "completed" } else { "needsAction" }));
    if let Some(date) = p.due_date.as_deref().and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()) {
        data.insert("due".into(), json!(format!("{}T00:00:00.000Z", date.format("%Y-%m-%d"))));
    }
    Value::Object(data)
}

pub fn external_id(list: &str, task: &str) -> String {
    format!("{list}:{task}")
}

/// `(list, task)` of an `external_id`; `Invalid` without both parts.
pub fn split_external_id(id: &str) -> Result<(&str, &str), ExportError> {
    match id.split_once(':') {
        Some((list, task)) if !list.is_empty() && !task.is_empty() => Ok((list, task)),
        _ => Err(ExportError::Invalid(format!("google tasks external id without list: {id}"))),
    }
}

/// `id` and `webViewLink` (the Tasks web app when missing).
pub fn parse_task(list: &str, v: &Value) -> Result<Exported, ExportError> {
    let Some(id) = v["id"].as_str().filter(|s| !s.is_empty()) else {
        return Err(ExportError::Invalid(format!("task response without id: {v}")));
    };
    let url = v["webViewLink"].as_str().filter(|s| !s.is_empty()).unwrap_or(WEB);
    Ok(Exported { external_id: external_id(list, id), external_url: url.into() })
}

/// `Gone` for a deleted task; `Status` from `status` and a parseable `updated`, else `Unknown`.
pub fn parse_task_status(v: &Value) -> RemoteState {
    if v["deleted"].as_bool() == Some(true) {
        return RemoteState::Gone;
    }
    let done = match v["status"].as_str() {
        Some("completed") => true,
        Some("needsAction") => false,
        _ => return RemoteState::Unknown,
    };
    match rfc3339_ms(v["updated"].as_str()) {
        Some(modified_at) => RemoteState::Status { done, modified_at },
        None => RemoteState::Unknown,
    }
}

/// Reopening clears `completed` too, or Google keeps the completion date.
pub fn status_body(done: bool) -> Value {
    if done {
        json!({ "status": "completed" })
    } else {
        json!({ "status": "needsAction", "completed": null })
    }
}

// ---------- HTTP ----------

pub async fn me(token: &str) -> Result<String, ExportError> {
    Ok(parse_me(&send(get(USERINFO, token)).await?))
}

pub async fn lists(token: &str) -> Result<Vec<TargetOption>, ExportError> {
    Ok(parse_lists(&send(get(&format!("{BASE}/users/@me/lists?maxResults=100"), token)).await?))
}

/// 404 maps to `TargetMissing` through `classify`; callers treat it as gone.
pub async fn task_status(token: &str, external_id: &str) -> Result<RemoteState, ExportError> {
    let (list, task) = split_external_id(external_id)?;
    Ok(parse_task_status(&send(get(&task_url(list, task), token)).await?))
}

pub async fn set_task_status(token: &str, external_id: &str, done: bool) -> Result<RemoteState, ExportError> {
    let (list, task) = split_external_id(external_id)?;
    let req = backend::http().patch(task_url(list, task)).bearer_auth(token).json(&status_body(done));
    Ok(parse_task_status(&send(req).await?))
}

/// Creates the task in the configured list. Google Tasks has no attachments or tags; the source
/// link rides in the notes.
pub async fn export(token: &str, cfg: &GoogleTasksConfig, p: &ExportPayload) -> Result<Exported, ExportError> {
    let url = format!("{BASE}/lists/{}/tasks", path_segment(&cfg.tasklist_id));
    let v = send(backend::http().post(url).bearer_auth(token).json(&task_body(p))).await?;
    parse_task(&cfg.tasklist_id, &v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> ExportPayload {
        ExportPayload {
            title: "Ship it".into(),
            notes: "Details".into(),
            done: false,
            due_date: Some("2026-10-17".into()),
            due_time: Some("09:30".into()),
            tags: vec!["To-do".into()],
            source_url: "copet://task/t1".into(),
            file: None,
        }
    }

    #[test]
    fn body_keeps_due_date_only() {
        assert_eq!(
            task_body(&payload()),
            json!({
                "title": "Ship it",
                "notes": "Details\n\nSource: copet://task/t1",
                "status": "needsAction",
                "due": "2026-10-17T00:00:00.000Z"
            })
        );
        let mut p = payload();
        p.notes = "  ".into();
        p.done = true;
        p.due_date = None;
        assert_eq!(task_body(&p), json!({ "title": "Ship it", "notes": "Source: copet://task/t1", "status": "completed" }));
    }

    #[test]
    fn body_fits_google_limits_and_keeps_source() {
        let mut p = payload();
        p.title = "é".repeat(MAX_TITLE + 10);
        p.notes = "ú".repeat(MAX_NOTES);
        let body = task_body(&p);
        assert_eq!(body["title"].as_str().unwrap().chars().count(), MAX_TITLE);
        let notes = body["notes"].as_str().unwrap();
        assert_eq!(notes.chars().count(), MAX_NOTES);
        assert!(notes.ends_with("\n\nSource: copet://task/t1"));
    }

    #[test]
    fn parses_responses() {
        assert_eq!(parse_me(&json!({ "email": "a@b.c", "name": "A" })), "a@b.c");
        assert_eq!(parse_me(&json!({ "email": " ", "name": "A" })), "A");
        assert_eq!(parse_me(&json!({})), "Google");
        assert_eq!(
            parse_lists(&json!({ "items": [{ "id": "L1", "title": "My Tasks" }, { "title": "no id" }] })),
            vec![TargetOption { id: "L1".into(), label: "My Tasks".into() }]
        );
        assert!(parse_lists(&json!({ "kind": "tasks#taskLists" })).is_empty());
        assert_eq!(
            parse_task("L1", &json!({ "id": "T9", "webViewLink": "https://tasks.google.com/task/T9" })).unwrap(),
            Exported { external_id: "L1:T9".into(), external_url: "https://tasks.google.com/task/T9".into() }
        );
        assert_eq!(parse_task("L1", &json!({ "id": "T9" })).unwrap().external_url, WEB);
        assert!(parse_task("L1", &json!({ "error": {} })).is_err());
    }

    #[test]
    fn external_id_round_trips() {
        let id = external_id("MTA3NDE3NzMzODQx", "dXpTZ0VKTE5D-_");
        assert_eq!(split_external_id(&id).unwrap(), ("MTA3NDE3NzMzODQx", "dXpTZ0VKTE5D-_"));
        assert!(split_external_id("T9").is_err());
        assert!(split_external_id(":T9").is_err());
        assert_eq!(task_url("L 1", "T/9"), format!("{BASE}/lists/L%201/tasks/T%2F9"));
    }

    #[test]
    fn parses_task_status() {
        let v = json!({ "id": "T9", "status": "completed", "updated": "2026-10-17T07:28:00.000Z" });
        assert_eq!(parse_task_status(&v), RemoteState::Status { done: true, modified_at: 1_792_222_080_000 });
        let v = json!({ "id": "T9", "status": "needsAction", "updated": "2026-10-17T07:28:00.000Z" });
        assert_eq!(parse_task_status(&v), RemoteState::Status { done: false, modified_at: 1_792_222_080_000 });
        assert_eq!(parse_task_status(&json!({ "status": "completed", "deleted": true })), RemoteState::Gone);
        assert_eq!(parse_task_status(&json!({ "status": "completed" })), RemoteState::Unknown);
        assert_eq!(status_body(true), json!({ "status": "completed" }));
        assert_eq!(status_body(false), json!({ "status": "needsAction", "completed": null }));
    }
}
