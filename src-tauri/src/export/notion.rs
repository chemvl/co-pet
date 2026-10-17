//! Notion API (`2026-03-11`, data sources). Requests are built and responses parsed by pure
//! functions; the async functions only send them.
use chrono::{Local, NaiveDate, NaiveTime, TimeZone};
use reqwest::RequestBuilder;
use serde_json::{json, Map, Value};

use super::{path_segment, rfc3339_ms, send, ExportError, ExportPayload, Exported, RemoteState};
use crate::backend;
use crate::db::search::fold;
use crate::i18n::Lang;
use crate::model::{NotionConfig, NotionMapping, NotionProp, NotionSchema, NotionStatusMap, TargetOption};

const BASE: &str = "https://api.notion.com/v1";
const VERSION: &str = "2026-03-11";
/// Notion's limit for one rich text content.
const TEXT_MAX: usize = 2000;
/// Note paragraphs per page; the source paragraph makes 100, Notion's children limit.
const MAX_NOTE_BLOCKS: usize = 99;

fn req(method: reqwest::Method, path: &str, token: &str) -> RequestBuilder {
    backend::http()
        .request(method, format!("{BASE}{path}"))
        .bearer_auth(token)
        .header("Notion-Version", VERSION)
}

// ---------- pure ----------

/// `bot.workspace_name`, else `name`, else "Notion".
pub fn parse_me(v: &Value) -> String {
    [&v["bot"]["workspace_name"], &v["name"]]
        .into_iter()
        .filter_map(Value::as_str)
        .find(|s| !s.trim().is_empty())
        .unwrap_or("Notion")
        .to_string()
}

fn plain_text(rich: &Value) -> String {
    rich.as_array()
        .map(|parts| parts.iter().filter_map(|p| p["plain_text"].as_str()).collect())
        .unwrap_or_default()
}

pub fn parse_data_sources(v: &Value, lang: Lang) -> Vec<TargetOption> {
    let untitled = match lang {
        Lang::En => "Untitled",
        Lang::Vi => "Không có tiêu đề",
    };
    v["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let id = r["id"].as_str()?.to_string();
            let label = plain_text(&r["title"]);
            Some(TargetOption { id, label: if label.trim().is_empty() { untitled.into() } else { label } })
        })
        .collect()
}

/// `(name, property)` pairs in the response's key order.
fn properties(v: &Value) -> Vec<(&str, &Value)> {
    v["properties"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(key, p)| (p["name"].as_str().unwrap_or(key), p))
        .collect()
}

fn kind(p: &Value) -> &str {
    p["type"].as_str().unwrap_or("")
}

fn option_names(p: &Value) -> Vec<String> {
    p[kind(p)]["options"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|o| o["name"].as_str().map(String::from))
        .collect()
}

/// The first option of a status group.
fn group_first(p: &Value, group: &Value) -> Option<String> {
    let first_id = group["option_ids"].as_array()?.first()?.as_str()?;
    p["status"]["options"]
        .as_array()?
        .iter()
        .find(|o| o["id"].as_str() == Some(first_id))?["name"]
        .as_str()
        .map(String::from)
}

fn status_map(p: &Value, name: &str) -> Option<NotionStatusMap> {
    let groups = p["status"]["groups"].as_array().cloned().unwrap_or_default();
    let named = |n: &str| groups.iter().find(|g| g["name"].as_str() == Some(n));
    let options = option_names(p);
    let todo = named("To-do")
        .or(groups.first())
        .and_then(|g| group_first(p, g))
        .or_else(|| options.first().cloned())?;
    let done = named("Complete")
        .or(groups.last())
        .and_then(|g| group_first(p, g))
        .or_else(|| options.last().cloned())?;
    Some(NotionStatusMap { prop: name.into(), kind: "status".into(), todo, done })
}

fn select_map(p: &Value, name: &str) -> Option<NotionStatusMap> {
    let options = option_names(p);
    let todo = options.first()?.clone();
    let done = options
        .iter()
        .find(|o| {
            let f = fold(o);
            f.contains("done") || f.contains("xong")
        })
        .or(options.last())?
        .clone();
    Some(NotionStatusMap { prop: name.into(), kind: "select".into(), todo, done })
}

/// The mapping the setup sheet starts from.
pub fn suggest_mapping(v: &Value) -> NotionMapping {
    let props = properties(v);
    let first = |k: &str| props.iter().find(|(_, p)| kind(p) == k).map(|(n, _)| n.to_string());
    let status = props
        .iter()
        .find(|(_, p)| kind(p) == "status")
        .and_then(|(n, p)| status_map(p, n))
        .or_else(|| {
            props
                .iter()
                .find(|(n, p)| {
                    let f = fold(n);
                    kind(p) == "select" && (f.contains("status") || f.contains("trang thai"))
                })
                .and_then(|(n, p)| select_map(p, n))
        });
    NotionMapping {
        title: first("title").unwrap_or_default(),
        status,
        due: first("date"),
        tags: first("multi_select"),
        source: first("url"),
    }
}

pub fn parse_schema(v: &Value) -> NotionSchema {
    NotionSchema {
        title: plain_text(&v["title"]),
        props: properties(v)
            .into_iter()
            .map(|(name, p)| NotionProp { name: name.into(), kind: kind(p).into(), options: option_names(p) })
            .collect(),
        suggested: suggest_mapping(v),
    }
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// `s` in pieces of at most `max` characters.
fn chunks(s: &str, max: usize) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    chars.chunks(max).map(|c| c.iter().collect()).collect()
}

fn paragraph(rich_text: Value) -> Value {
    json!({ "object": "block", "type": "paragraph", "paragraph": { "rich_text": rich_text } })
}

/// `YYYY-MM-DD`, or the local date+time with its UTC offset.
fn date_start(date: &str, time: Option<&str>) -> Option<String> {
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let Some(t) = time.and_then(|t| NaiveTime::parse_from_str(t, "%H:%M").ok()) else {
        return Some(d.format("%Y-%m-%d").to_string());
    };
    let local = Local.from_local_datetime(&d.and_time(t)).earliest()?;
    Some(local.format("%Y-%m-%dT%H:%M:00%:z").to_string())
}

fn status_key(s: &NotionStatusMap) -> &'static str {
    if s.kind == "select" { "select" } else { "status" }
}

/// The status/select property value for option `done` or `todo`.
fn status_value(s: &NotionStatusMap, done: bool) -> Value {
    json!({ (status_key(s)): { "name": if done { &s.done } else { &s.todo } } })
}

/// `Gone` when trashed/archived; `Unknown` without a parseable `last_edited_time` or when the
/// mapped property is missing or of another type. A null select counts as open.
pub fn parse_page_status(v: &Value, map: &NotionStatusMap) -> RemoteState {
    if v["in_trash"].as_bool() == Some(true) || v["is_archived"].as_bool() == Some(true) {
        return RemoteState::Gone;
    }
    let Some(modified_at) = rfc3339_ms(v["last_edited_time"].as_str()) else {
        return RemoteState::Unknown;
    };
    let key = status_key(map);
    let prop = &v["properties"][&map.prop];
    if prop["type"].as_str() != Some(key) {
        return RemoteState::Unknown;
    }
    RemoteState::Status { done: prop[key]["name"].as_str() == Some(map.done.as_str()), modified_at }
}

/// The `PATCH /pages/{id}` body that sets only the status property.
pub fn status_patch_body(map: &NotionStatusMap, done: bool) -> Value {
    json!({ "properties": { (map.prop.clone()): status_value(map, done) } })
}

/// The `POST /pages` body. Unmapped properties are omitted; the source becomes a last
/// paragraph only when no URL property is mapped.
pub fn page_body(cfg: &NotionConfig, p: &ExportPayload) -> Value {
    let m = &cfg.mapping;
    let mut props = Map::new();
    props.insert(m.title.clone(), json!({ "title": [{ "text": { "content": truncate(&p.title, TEXT_MAX) } }] }));
    if let Some(s) = &m.status {
        props.insert(s.prop.clone(), status_value(s, p.done));
    }
    if let (Some(prop), Some(start)) = (&m.due, p.due_date.as_deref().and_then(|d| date_start(d, p.due_time.as_deref()))) {
        props.insert(prop.clone(), json!({ "date": { "start": start } }));
    }
    if let Some(prop) = &m.tags {
        // Commas are not allowed in multi-select option names.
        let tags: Vec<Value> = p.tags.iter().map(|t| json!({ "name": truncate(&t.replace(',', " "), 100) })).collect();
        props.insert(prop.clone(), json!({ "multi_select": tags }));
    }
    if let Some(prop) = &m.source {
        props.insert(prop.clone(), json!({ "url": p.source_url }));
    }
    let mut children: Vec<Value> = chunks(&p.notes, TEXT_MAX)
        .into_iter()
        .filter(|c| !c.trim().is_empty())
        .take(MAX_NOTE_BLOCKS)
        .map(|c| paragraph(json!([{ "type": "text", "text": { "content": c } }])))
        .collect();
    if m.source.is_none() {
        let mut link = json!({ "content": p.source_url });
        // Notion only links web URLs; `copet://` stays plain text.
        if p.source_url.starts_with("http://") || p.source_url.starts_with("https://") {
            link["link"] = json!({ "url": p.source_url });
        }
        children.push(paragraph(json!([
            { "type": "text", "text": { "content": "Source: " } },
            { "type": "text", "text": link }
        ])));
    }
    json!({
        "parent": { "type": "data_source_id", "data_source_id": cfg.data_source_id },
        "properties": props,
        "children": children,
    })
}

pub fn parse_page(v: &Value) -> Result<Exported, ExportError> {
    match (v["id"].as_str(), v["url"].as_str()) {
        (Some(id), Some(url)) => Ok(Exported { external_id: id.into(), external_url: url.into() }),
        _ => Err(ExportError::Invalid(format!("page response without id/url: {v}"))),
    }
}

// ---------- HTTP ----------

pub async fn me(token: &str) -> Result<String, ExportError> {
    Ok(parse_me(&send(req(reqwest::Method::GET, "/users/me", token)).await?))
}

pub async fn data_sources(token: &str, lang: Lang) -> Result<Vec<TargetOption>, ExportError> {
    let body = json!({ "filter": { "property": "object", "value": "data_source" }, "page_size": 100 });
    let v = send(req(reqwest::Method::POST, "/search", token).json(&body)).await?;
    Ok(parse_data_sources(&v, lang))
}

pub async fn schema(token: &str, id: &str) -> Result<NotionSchema, ExportError> {
    let path = format!("/data_sources/{}", path_segment(id));
    Ok(parse_schema(&send(req(reqwest::Method::GET, &path, token)).await?))
}

pub async fn export(token: &str, cfg: &NotionConfig, p: &ExportPayload) -> Result<Exported, ExportError> {
    let v = send(req(reqwest::Method::POST, "/pages", token).json(&page_body(cfg, p))).await?;
    parse_page(&v)
}

fn page_path(page_id: &str) -> String {
    format!("/pages/{}", path_segment(page_id))
}

#[allow(dead_code)] // called by export::status (Task 6.2)
pub async fn page_status(token: &str, page_id: &str, map: &NotionStatusMap) -> Result<RemoteState, ExportError> {
    let r = req(reqwest::Method::GET, &page_path(page_id), token).query(&[("filter_properties", map.prop.as_str())]);
    Ok(parse_page_status(&send(r).await?, map))
}

#[allow(dead_code)] // called by export::status (Task 6.2)
pub async fn set_page_status(token: &str, page_id: &str, map: &NotionStatusMap, done: bool) -> Result<RemoteState, ExportError> {
    let r = req(reqwest::Method::PATCH, &page_path(page_id), token)
        .query(&[("filter_properties", map.prop.as_str())])
        .json(&status_patch_body(map, done));
    Ok(parse_page_status(&send(r).await?, map))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        json!({
            "object": "data_source",
            "id": "ds1",
            "title": [{ "plain_text": "Frontend " }, { "plain_text": "sprint" }],
            "properties": {
                "Name": { "id": "title", "name": "Name", "type": "title", "title": {} },
                "Status": { "id": "s", "name": "Status", "type": "status", "status": {
                    "options": [
                        { "id": "o1", "name": "Not started" },
                        { "id": "o2", "name": "In progress" },
                        { "id": "o3", "name": "Done" }
                    ],
                    "groups": [
                        { "id": "g1", "name": "To-do", "option_ids": ["o1"] },
                        { "id": "g2", "name": "In progress", "option_ids": ["o2"] },
                        { "id": "g3", "name": "Complete", "option_ids": ["o3"] }
                    ]
                } },
                "Due": { "id": "d", "name": "Due", "type": "date", "date": {} },
                "Tags": { "id": "t", "name": "Tags", "type": "multi_select", "multi_select": {
                    "options": [{ "id": "x", "name": "web" }]
                } },
                "URL": { "id": "u", "name": "URL", "type": "url", "url": {} }
            }
        })
    }

    #[test]
    fn suggest_mapping_from_schema() {
        let schema = parse_schema(&fixture());
        assert_eq!(schema.title, "Frontend sprint");
        assert_eq!(
            schema.suggested,
            NotionMapping {
                title: "Name".into(),
                status: Some(NotionStatusMap {
                    prop: "Status".into(),
                    kind: "status".into(),
                    todo: "Not started".into(),
                    done: "Done".into(),
                }),
                due: Some("Due".into()),
                tags: Some("Tags".into()),
                source: Some("URL".into()),
            }
        );
        let status = schema.props.iter().find(|p| p.name == "Status").unwrap();
        assert_eq!(status.options, vec!["Not started", "In progress", "Done"]);
    }

    #[test]
    fn mapping_follows_column_order() {
        let v: Value = serde_json::from_str(
            r#"{ "properties": {
                "Zeta": { "name": "Zeta", "type": "title", "title": {} },
                "Start": { "name": "Start", "type": "date", "date": {} },
                "Deadline": { "name": "Deadline", "type": "date", "date": {} },
                "Area": { "name": "Area", "type": "multi_select", "multi_select": { "options": [] } }
            } }"#,
        )
        .unwrap();
        let schema = parse_schema(&v);
        let names: Vec<&str> = schema.props.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["Zeta", "Start", "Deadline", "Area"]);
        assert_eq!(schema.suggested.due.as_deref(), Some("Start"));
    }

    #[test]
    fn select_status_fallback() {
        let v = json!({ "properties": {
            "Tên": { "name": "Tên", "type": "title", "title": {} },
            "Trạng thái": { "name": "Trạng thái", "type": "select", "select": {
                "options": [{ "name": "Mới" }, { "name": "Đã xong" }, { "name": "Hủy" }]
            } }
        } });
        let m = suggest_mapping(&v);
        assert_eq!(m.title, "Tên");
        let s = m.status.unwrap();
        assert_eq!((s.prop.as_str(), s.kind.as_str(), s.todo.as_str(), s.done.as_str()), ("Trạng thái", "select", "Mới", "Đã xong"));
        assert_eq!((m.due, m.tags, m.source), (None, None, None));
    }

    fn payload() -> ExportPayload {
        ExportPayload {
            title: "Ship it".into(),
            notes: "Line one".into(),
            done: true,
            due_date: Some("2026-10-17".into()),
            due_time: None,
            tags: vec!["To-do".into(), "a,b".into()],
            source_url: "https://example.com/x".into(),
            file: None,
        }
    }

    #[test]
    fn page_body_omits_unmapped_props() {
        let cfg = NotionConfig {
            data_source_id: "ds1".into(),
            mapping: NotionMapping { title: "Name".into(), status: None, due: None, tags: None, source: None },
        };
        let body = page_body(&cfg, &payload());
        assert_eq!(body["parent"], json!({ "type": "data_source_id", "data_source_id": "ds1" }));
        let props = body["properties"].as_object().unwrap();
        assert_eq!(props.keys().collect::<Vec<_>>(), vec!["Name"]);
        assert_eq!(props["Name"], json!({ "title": [{ "text": { "content": "Ship it" } }] }));
        let children = body["children"].as_array().unwrap();
        assert_eq!(children.len(), 2);
        let source = &children[1]["paragraph"]["rich_text"];
        assert_eq!(source[0]["text"]["content"], "Source: ");
        assert_eq!(source[1]["text"]["link"]["url"], "https://example.com/x");
    }

    #[test]
    fn page_body_maps_every_prop() {
        let cfg = NotionConfig { data_source_id: "ds1".into(), mapping: suggest_mapping(&fixture()) };
        let mut p = payload();
        p.notes = "x".repeat(TEXT_MAX * 120);
        let body = page_body(&cfg, &p);
        let props = &body["properties"];
        assert_eq!(props["Status"], json!({ "status": { "name": "Done" } }));
        assert_eq!(props["Due"], json!({ "date": { "start": "2026-10-17" } }));
        assert_eq!(props["Tags"], json!({ "multi_select": [{ "name": "To-do" }, { "name": "a b" }] }));
        assert_eq!(props["URL"], json!({ "url": "https://example.com/x" }));
        // Source is mapped: no source paragraph, notes capped at 99 blocks.
        assert_eq!(body["children"].as_array().unwrap().len(), MAX_NOTE_BLOCKS);
        let d = date_start("2026-10-17", Some("09:30")).unwrap();
        assert!(d.starts_with("2026-10-17T09:30:00") && d.len() == 25, "{d}");
    }

    #[test]
    fn parses_me_and_sources() {
        assert_eq!(parse_me(&json!({ "bot": { "workspace_name": "Acme" }, "name": "Bot" })), "Acme");
        assert_eq!(parse_me(&json!({ "bot": {}, "name": "Bot" })), "Bot");
        assert_eq!(parse_me(&json!({})), "Notion");
        let v = json!({ "results": [{ "id": "a", "title": [{ "plain_text": "Tasks" }] }, { "id": "b", "title": [] }] });
        assert_eq!(
            parse_data_sources(&v, Lang::Vi),
            vec![
                TargetOption { id: "a".into(), label: "Tasks".into() },
                TargetOption { id: "b".into(), label: "Không có tiêu đề".into() }
            ]
        );
        assert_eq!(
            parse_page(&json!({ "id": "p1", "url": "https://notion.so/p1" })).unwrap(),
            Exported { external_id: "p1".into(), external_url: "https://notion.so/p1".into() }
        );
    }

    fn status_map(kind: &str) -> NotionStatusMap {
        NotionStatusMap { prop: "Status".into(), kind: kind.into(), todo: "Not started".into(), done: "Done".into() }
    }

    #[test]
    fn parses_page_status() {
        let m = status_map("status");
        let page = |name: &str| {
            json!({ "id": "p", "in_trash": false, "is_archived": false, "last_edited_time": "2026-10-17T07:28:00.000Z",
                "properties": { "Status": { "id": "s", "type": "status", "status": { "name": name } } } })
        };
        assert_eq!(parse_page_status(&page("Done"), &m), RemoteState::Status { done: true, modified_at: 1_792_222_080_000 });
        assert_eq!(parse_page_status(&page("In progress"), &m), RemoteState::Status { done: false, modified_at: 1_792_222_080_000 });
        let mut v = page("Done");
        v["in_trash"] = json!(true);
        assert_eq!(parse_page_status(&v, &m), RemoteState::Gone);
        let mut v = page("Done");
        v["is_archived"] = json!(true);
        assert_eq!(parse_page_status(&v, &m), RemoteState::Gone);
        let mut v = page("Done");
        v["properties"] = json!({ "Name": { "type": "title", "title": [] } });
        assert_eq!(parse_page_status(&v, &m), RemoteState::Unknown);
        let mut v = page("Done");
        v["last_edited_time"] = json!("yesterday");
        assert_eq!(parse_page_status(&v, &m), RemoteState::Unknown);
        let v = json!({ "last_edited_time": "2026-10-17T07:28:00.000Z",
            "properties": { "Status": { "type": "select", "select": null } } });
        assert_eq!(parse_page_status(&v, &status_map("select")), RemoteState::Status { done: false, modified_at: 1_792_222_080_000 });
    }

    #[test]
    fn status_patch_body_by_kind() {
        assert_eq!(status_patch_body(&status_map("status"), true), json!({ "properties": { "Status": { "status": { "name": "Done" } } } }));
        let m = NotionStatusMap { prop: "Trạng thái".into(), kind: "select".into(), todo: "Mới".into(), done: "Xong".into() };
        assert_eq!(status_patch_body(&m, false), json!({ "properties": { "Trạng thái": { "select": { "name": "Mới" } } } }));
    }
}
