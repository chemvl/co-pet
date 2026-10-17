//! Asana API. Requests are built and responses parsed by pure functions; the async functions
//! only send them.
use chrono::{Local, NaiveDate, NaiveTime, TimeZone, Utc};
use reqwest::multipart::{Form, Part};
use reqwest::RequestBuilder;
use serde_json::{json, Map, Value};

use super::{path_segment, rfc3339_ms, send, ExportError, ExportPayload, Exported, RemoteState};
use crate::backend;
use crate::model::{AsanaConfig, TargetOption};

const BASE: &str = "https://app.asana.com/api/1.0";

fn get(path: &str, token: &str) -> RequestBuilder {
    backend::http().get(format!("{BASE}{path}")).bearer_auth(token)
}

fn post(path: &str, token: &str) -> RequestBuilder {
    backend::http().post(format!("{BASE}{path}")).bearer_auth(token)
}

fn put(path: &str, token: &str) -> RequestBuilder {
    backend::http().put(format!("{BASE}{path}")).bearer_auth(token)
}

const STATUS_FIELDS: &str = "opt_fields=completed,modified_at";

// ---------- pure ----------

/// `data.email`, else `data.name`, else "Asana".
pub fn parse_me(v: &Value) -> String {
    [&v["data"]["email"], &v["data"]["name"]]
        .into_iter()
        .filter_map(Value::as_str)
        .find(|s| !s.trim().is_empty())
        .unwrap_or("Asana")
        .to_string()
}

/// `data[] {gid, name}`.
pub fn parse_options(v: &Value) -> Vec<TargetOption> {
    v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|o| Some(TargetOption { id: o["gid"].as_str()?.into(), label: o["name"].as_str().unwrap_or("").into() }))
        .collect()
}

/// `due_at` (UTC) for a local date and time.
fn due_at(date: NaiveDate, time: NaiveTime) -> Option<String> {
    let local = Local.from_local_datetime(&date.and_time(time)).earliest()?;
    Some(local.with_timezone(&Utc).format("%Y-%m-%dT%H:%M:%S.000Z").to_string())
}

/// The `POST /tasks` body.
pub fn task_body(cfg: &AsanaConfig, p: &ExportPayload) -> Value {
    let notes = if p.notes.trim().is_empty() {
        format!("Source: {}", p.source_url)
    } else {
        format!("{}\n\nSource: {}", p.notes, p.source_url)
    };
    let mut data = Map::new();
    data.insert("name".into(), json!(p.title));
    data.insert("notes".into(), json!(notes));
    if let Some(date) = p.due_date.as_deref().and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()) {
        match p.due_time.as_deref().and_then(|t| NaiveTime::parse_from_str(t, "%H:%M").ok()).and_then(|t| due_at(date, t)) {
            Some(at) => data.insert("due_at".into(), json!(at)),
            None => data.insert("due_on".into(), json!(date.format("%Y-%m-%d").to_string())),
        };
    }
    data.insert("assignee".into(), json!("me"));
    data.insert("workspace".into(), json!(cfg.workspace_gid));
    data.insert("completed".into(), json!(p.done));
    match &cfg.section_gid {
        Some(section) => data.insert("memberships".into(), json!([{ "project": cfg.project_gid, "section": section }])),
        None => data.insert("projects".into(), json!([cfg.project_gid])),
    };
    json!({ "data": data })
}

pub fn parse_task(v: &Value) -> Result<Exported, ExportError> {
    match (v["data"]["gid"].as_str(), v["data"]["permalink_url"].as_str()) {
        (Some(gid), Some(url)) => Ok(Exported { external_id: gid.into(), external_url: url.into() }),
        _ => Err(ExportError::Invalid(format!("task response without gid/permalink_url: {v}"))),
    }
}

/// `Status` from `data.completed` and a parseable `data.modified_at`, else `Unknown`.
pub fn parse_task_status(v: &Value) -> RemoteState {
    match (v["data"]["completed"].as_bool(), rfc3339_ms(v["data"]["modified_at"].as_str())) {
        (Some(done), Some(modified_at)) => RemoteState::Status { done, modified_at },
        _ => RemoteState::Unknown,
    }
}

pub fn status_body(done: bool) -> Value {
    json!({ "data": { "completed": done } })
}

// ---------- HTTP ----------

pub async fn me(token: &str) -> Result<String, ExportError> {
    Ok(parse_me(&send(get("/users/me", token)).await?))
}

pub async fn workspaces(token: &str) -> Result<Vec<TargetOption>, ExportError> {
    Ok(parse_options(&send(get("/workspaces?limit=100", token)).await?))
}

pub async fn projects(token: &str, workspace_gid: &str) -> Result<Vec<TargetOption>, ExportError> {
    let path = format!("/workspaces/{}/projects?archived=false&limit=100", path_segment(workspace_gid));
    Ok(parse_options(&send(get(&path, token)).await?))
}

pub async fn sections(token: &str, project_gid: &str) -> Result<Vec<TargetOption>, ExportError> {
    let path = format!("/projects/{}/sections?limit=100", path_segment(project_gid));
    Ok(parse_options(&send(get(&path, token)).await?))
}

/// 404 maps to `TargetMissing` through `classify`; callers treat it as gone.
#[allow(dead_code)] // called by export::status (Task 6.2)
pub async fn task_status(token: &str, gid: &str) -> Result<RemoteState, ExportError> {
    let path = format!("/tasks/{}?{STATUS_FIELDS}", path_segment(gid));
    Ok(parse_task_status(&send(get(&path, token)).await?))
}

#[allow(dead_code)] // called by export::status (Task 6.2)
pub async fn set_task_status(token: &str, gid: &str, done: bool) -> Result<RemoteState, ExportError> {
    let path = format!("/tasks/{}?{STATUS_FIELDS}", path_segment(gid));
    Ok(parse_task_status(&send(put(&path, token).json(&status_body(done))).await?))
}

/// Creates the task, then attaches the source file; an attachment failure is only logged.
pub async fn export(token: &str, cfg: &AsanaConfig, p: &ExportPayload) -> Result<Exported, ExportError> {
    let v = send(post("/tasks?opt_fields=gid,permalink_url", token).json(&task_body(cfg, p))).await?;
    let exported = parse_task(&v)?;
    if let Some((path, name)) = &p.file {
        match tokio::fs::read(path).await {
            Ok(bytes) => {
                let form = Form::new()
                    .text("parent", exported.external_id.clone())
                    .part("file", Part::bytes(bytes).file_name(name.clone()));
                if let Err(e) = send(post("/attachments", token).multipart(form)).await {
                    eprintln!("[export] asana attachment failed: {e:?}");
                }
            }
            Err(e) => eprintln!("[export] asana attachment read failed: {e}"),
        }
    }
    Ok(exported)
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
            due_time: None,
            tags: vec![],
            source_url: "copet://task/t1".into(),
            file: None,
        }
    }

    #[test]
    fn body_uses_memberships_with_section() {
        let mut cfg = AsanaConfig { workspace_gid: "w1".into(), project_gid: "p1".into(), section_gid: Some("s1".into()) };
        let body = task_body(&cfg, &payload());
        assert_eq!(
            body,
            json!({ "data": {
                "name": "Ship it",
                "notes": "Details\n\nSource: copet://task/t1",
                "due_on": "2026-10-17",
                "assignee": "me",
                "workspace": "w1",
                "completed": false,
                "memberships": [{ "project": "p1", "section": "s1" }]
            } })
        );
        cfg.section_gid = None;
        let mut p = payload();
        p.due_time = Some("09:30".into());
        let body = task_body(&cfg, &p);
        assert_eq!(body["data"]["projects"], json!(["p1"]));
        assert!(body["data"].get("memberships").is_none() && body["data"].get("due_on").is_none());
        let at = body["data"]["due_at"].as_str().unwrap();
        assert!(at.ends_with(":00.000Z") && at.len() == 24, "{at}");
    }

    #[test]
    fn parses_responses() {
        assert_eq!(parse_me(&json!({ "data": { "email": "a@b.c", "name": "A" } })), "a@b.c");
        assert_eq!(parse_me(&json!({ "data": {} })), "Asana");
        assert_eq!(
            parse_options(&json!({ "data": [{ "gid": "1", "name": "Co-pet" }, { "name": "no gid" }] })),
            vec![TargetOption { id: "1".into(), label: "Co-pet".into() }]
        );
        assert_eq!(
            parse_task(&json!({ "data": { "gid": "9", "permalink_url": "https://app.asana.com/0/1/9" } })).unwrap(),
            Exported { external_id: "9".into(), external_url: "https://app.asana.com/0/1/9".into() }
        );
        assert!(parse_task(&json!({ "errors": [] })).is_err());
    }

    #[test]
    fn parses_task_status() {
        let v = json!({ "data": { "gid": "9", "completed": true, "modified_at": "2026-10-17T07:28:00.000Z" } });
        assert_eq!(parse_task_status(&v), RemoteState::Status { done: true, modified_at: 1_792_222_080_000 });
        assert_eq!(parse_task_status(&json!({ "data": { "gid": "9", "completed": true } })), RemoteState::Unknown);
        assert_eq!(status_body(false), json!({ "data": { "completed": false } }));
    }
}
