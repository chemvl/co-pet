//! Notion and Asana export: the providers, the payload sent for a task, keychain tokens (with
//! refresh through the backend), the shared HTTP status mapping, and what the integration
//! commands do. `queue` runs the jobs; `oauth` connects an account.
//!
//! A new exporter = one module + one `Provider` variant + the match arms below.
pub mod asana;
pub mod notion;
pub mod oauth;
pub mod queue;

use std::path::{Path, PathBuf};
use std::str::FromStr;

use reqwest::RequestBuilder;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::backend;
use crate::db::{categories, items, settings, tasks};
use crate::error::{AppError, AppResult};
use crate::events;
use crate::i18n::Lang;
use crate::model::{AsanaConfig, ExportTarget, ItemRow, NotionConfig, ProviderStatus, TaskExportState};
use crate::secrets::{self, ProviderToken};

/// Source files above this size are not attached.
const MAX_FILE_BYTES: i64 = 100 * 1024 * 1024;
const MAX_TAGS: usize = 5;
/// Refresh an OAuth token this long before it expires.
const REFRESH_MARGIN_MS: i64 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Notion,
    Asana,
}

impl Provider {
    pub const ALL: [Provider; 2] = [Provider::Notion, Provider::Asana];

    /// Also the keychain account and the `export_targets.id`.
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Notion => "notion",
            Provider::Asana => "asana",
        }
    }
}

impl FromStr for Provider {
    type Err = AppError;

    fn from_str(s: &str) -> AppResult<Self> {
        match s {
            "notion" => Ok(Provider::Notion),
            "asana" => Ok(Provider::Asana),
            _ => Err(AppError::Code("invalid")),
        }
    }
}

/// What a task becomes on the other side.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportPayload {
    pub title: String,
    pub notes: String,
    pub done: bool,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub tags: Vec<String>,
    pub source_url: String,
    /// Absolute path and file name of the source item's library file.
    pub file: Option<(PathBuf, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Exported {
    pub external_id: String,
    pub external_url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExportError {
    /// Seconds to wait.
    RateLimited(u64),
    Retryable,
    Unauthorized,
    TargetMissing,
    Invalid(String),
}

/// For the commands that call the providers directly (pickers, token check).
impl From<ExportError> for AppError {
    fn from(e: ExportError) -> Self {
        match e {
            ExportError::RateLimited(_) => AppError::Code("rateLimited"),
            ExportError::Retryable => AppError::Code("network"),
            ExportError::Unauthorized => AppError::Code("unauthorized"),
            ExportError::TargetMissing => AppError::Code("targetMissing"),
            ExportError::Invalid(msg) => {
                eprintln!("[export] rejected: {msg}");
                AppError::Code("invalid")
            }
        }
    }
}

// ---------- HTTP ----------

/// Sends `req`; the JSON body of a 2xx response, else the mapped error.
pub(crate) async fn send(req: RequestBuilder) -> Result<Value, ExportError> {
    let res = req.send().await.map_err(|e| {
        eprintln!("[export] request failed: {e}");
        ExportError::Retryable
    })?;
    let status = res.status().as_u16();
    if res.status().is_success() {
        return res.json::<Value>().await.map_err(|e| {
            eprintln!("[export] response body: {e}");
            ExportError::Retryable
        });
    }
    let retry_after = res
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let body = res.text().await.unwrap_or_default();
    Err(classify(status, retry_after.as_deref(), body))
}

/// Retry-After when the header is missing or not a number of seconds.
const DEFAULT_RETRY_AFTER_S: u64 = 30;

/// `429` (and Notion's `529`) → `RateLimited(Retry-After, at least 1 s, default 30 s)`;
/// `401`/`403` → `Unauthorized`; `404` → `TargetMissing`; other `4xx` → `Invalid(body)`; the
/// rest → `Retryable`.
pub fn classify(status: u16, retry_after: Option<&str>, body: String) -> ExportError {
    match status {
        429 | 529 => ExportError::RateLimited(
            retry_after.and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(DEFAULT_RETRY_AFTER_S).max(1),
        ),
        401 | 403 => ExportError::Unauthorized,
        404 => ExportError::TargetMissing,
        400..=499 => ExportError::Invalid(body),
        _ => ExportError::Retryable,
    }
}

/// Characters a URL path segment keeps as is: RFC 3986 unreserved.
const SEGMENT: &percent_encoding::AsciiSet =
    &percent_encoding::NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~');

/// `s` percent-encoded as one URL path segment (ids come from the UI).
pub(crate) fn path_segment(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, SEGMENT).to_string()
}

// ---------- tokens ----------

/// A token as the backend's `/oauth/claim` and `/oauth/refresh` return it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BackendToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Epoch ms.
    pub expires_at: Option<i64>,
}

fn stored(p: Provider) -> AppResult<ProviderToken> {
    secrets::get_json(p.as_str())?.ok_or(AppError::Code("notConnected"))
}

fn needs_refresh(t: &ProviderToken, now: i64) -> bool {
    t.refresh_token.is_some() && t.expires_at.is_some_and(|at| at < now + REFRESH_MARGIN_MS)
}

/// The refreshed token; a `null` refresh token in the response keeps the stored one (Asana).
fn merge_refreshed(old: ProviderToken, fresh: BackendToken) -> ProviderToken {
    ProviderToken {
        access_token: fresh.access_token,
        refresh_token: fresh.refresh_token.or(old.refresh_token),
        expires_at: fresh.expires_at,
        needs_reconnect: false,
        ..old
    }
}

/// The keychain access token, refreshed through the backend `/oauth/refresh` when it expires
/// within 60 s. `notConnected` when there is none.
pub async fn token(p: Provider) -> AppResult<String> {
    let t = stored(p)?;
    if needs_refresh(&t, crate::db::now_ms()) {
        return Ok(refresh(p, t).await?.access_token);
    }
    Ok(t.access_token)
}

/// Runs the refresh grant through the backend and stores the result. A `4xx` from the function
/// (revoked grant) is `unauthorized`.
async fn refresh(p: Provider, old: ProviderToken) -> AppResult<ProviderToken> {
    let b = backend::config().ok_or(AppError::Code("notConfigured"))?;
    let refresh_token = old.refresh_token.clone().ok_or(AppError::Code("unauthorized"))?;
    let res = backend::http()
        .post(format!("{}/functions/v1/oauth/refresh", b.url))
        .header("apikey", b.key)
        .json(&json!({ "provider": p.as_str(), "refreshToken": refresh_token }))
        .send()
        .await?;
    if res.status().is_client_error() {
        return Err(AppError::Code("unauthorized"));
    }
    let fresh: BackendToken = res.error_for_status()?.json().await?;
    let t = merge_refreshed(old, fresh);
    secrets::set_json(p.as_str(), &t)?;
    Ok(t)
}

fn token_error(e: AppError) -> ExportError {
    match e {
        AppError::Code("notConnected" | "unauthorized" | "notConfigured") => ExportError::Unauthorized,
        AppError::Http(_) => ExportError::Retryable,
        other => ExportError::Invalid(other.to_string()),
    }
}

/// Flags the keychain token so the UI asks to reconnect.
pub fn mark_needs_reconnect(p: Provider) -> AppResult<()> {
    if let Some(mut t) = secrets::get_json::<ProviderToken>(p.as_str())? {
        t.needs_reconnect = true;
        secrets::set_json(p.as_str(), &t)?;
    }
    Ok(())
}

/// The account label `p` reports for `token`.
pub async fn me(p: Provider, token: &str) -> Result<String, ExportError> {
    match p {
        Provider::Notion => notion::me(token).await,
        Provider::Asana => asana::me(token).await,
    }
}

// ---------- export ----------

async fn export_with(p: Provider, token: &str, target: &ExportTarget, payload: &ExportPayload) -> Result<Exported, ExportError> {
    let invalid = |e: serde_json::Error| ExportError::Invalid(format!("target config: {e}"));
    match p {
        Provider::Notion => {
            let cfg: NotionConfig = serde_json::from_value(target.config.clone()).map_err(invalid)?;
            notion::export(token, &cfg, payload).await
        }
        Provider::Asana => {
            let cfg: AsanaConfig = serde_json::from_value(target.config.clone()).map_err(invalid)?;
            asana::export(token, &cfg, payload).await
        }
    }
}

/// Creates the page/task. On `Unauthorized` with a refresh token, refreshes once and retries.
pub async fn export(p: Provider, target: &ExportTarget, payload: &ExportPayload) -> Result<Exported, ExportError> {
    let access = token(p).await.map_err(token_error)?;
    match export_with(p, &access, target, payload).await {
        Err(ExportError::Unauthorized) => {
            let t = stored(p).map_err(token_error)?;
            if t.refresh_token.is_none() {
                return Err(ExportError::Unauthorized);
            }
            let fresh = refresh(p, t).await.map_err(token_error)?;
            export_with(p, &fresh.access_token, target, payload).await
        }
        r => r,
    }
}

/// The source item's `url` for a `url` item, else `copet://item/<id>` with a live source item,
/// else `copet://task/<id>`.
fn source_url(task_id: &str, source: Option<&ItemRow>) -> String {
    match source {
        Some(item) if item.kind == "url" && item.url.as_deref().is_some_and(|u| !u.is_empty()) => {
            item.url.clone().unwrap_or_default()
        }
        Some(item) => format!("copet://item/{}", item.id),
        None => format!("copet://task/{task_id}"),
    }
}

/// The payload of live task `task_id`: tags = the category's name in `lang` plus the source
/// item's tags (max 5); `file` = the source item's library file when ≤ 100 MB.
pub fn payload(conn: &Connection, lang: Lang, library: &Path, task_id: &str) -> AppResult<ExportPayload> {
    let detail = tasks::get(conn, task_id)?;
    let t = &detail.task;
    let source = match &t.source_item_id {
        Some(id) => Some(items::get_row(conn, id)?).filter(|i| i.deleted_at.is_none()),
        None => None,
    };
    let mut tags = vec![categories::display_name(conn, &t.category_id, lang)?];
    for tag in source.iter().flat_map(|i| &i.tags) {
        if tags.len() == MAX_TAGS {
            break;
        }
        if !tag.trim().is_empty() && !tags.contains(tag) {
            tags.push(tag.clone());
        }
    }
    let file = source.as_ref().and_then(|i| {
        let rel = i.file_path.as_ref()?;
        if i.size_bytes.is_some_and(|n| n > MAX_FILE_BYTES) {
            return None;
        }
        let path = library.join(rel);
        let name = i.file_name.clone().or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()))?;
        Some((path, name))
    });
    Ok(ExportPayload {
        title: t.title.clone(),
        notes: detail.notes.clone(),
        done: t.status == "done",
        due_date: t.due_date.clone(),
        due_time: t.due_time.clone(),
        tags,
        source_url: source_url(&t.id, source.as_ref()),
        file,
    })
}

// ---------- integration state (commands) ----------

pub fn device_id(conn: &Connection) -> AppResult<String> {
    settings::get(conn, "sync.deviceId")
}

pub fn target(conn: &Connection, p: Provider) -> AppResult<Option<ExportTarget>> {
    let row = conn
        .query_row(
            "SELECT id, provider, account_label, target_label, config FROM export_targets
             WHERE id = ?1 AND deleted_at IS NULL",
            [p.as_str()],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?;
    row.map(|(id, provider, account_label, target_label, config)| {
        Ok(ExportTarget { id, provider, account_label, target_label, config: serde_json::from_str(&config)? })
    })
    .transpose()
}

/// `ProviderStatus` from the keychain token and this device's jobs.
pub fn status(conn: &Connection, p: Provider, token: Option<ProviderToken>) -> AppResult<ProviderStatus> {
    let device = device_id(conn)?;
    let (queued, failed, next_attempt_at) = conn.query_row(
        "SELECT COALESCE(SUM(state IN ('queued','exporting')), 0), COALESCE(SUM(state = 'failed'), 0),
                MIN(CASE WHEN state = 'queued' THEN next_attempt_at END)
         FROM task_exports WHERE provider = ?1 AND device_id = ?2 AND deleted_at IS NULL",
        params![p.as_str(), device],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<i64>>(2)?)),
    )?;
    let last_error = conn
        .query_row(
            "SELECT error FROM task_exports
             WHERE provider = ?1 AND device_id = ?2 AND deleted_at IS NULL AND state != 'exported' AND error IS NOT NULL
             ORDER BY updated_at DESC LIMIT 1",
            params![p.as_str(), device],
            |r| r.get(0),
        )
        .optional()?;
    Ok(ProviderStatus {
        provider: p.as_str().into(),
        connected: token.is_some(),
        method: token.as_ref().map(|t| t.method.clone()),
        account_label: token.as_ref().map(|t| t.account_label.clone()),
        needs_reconnect: token.as_ref().is_some_and(|t| t.needs_reconnect),
        target: target(conn, p)?,
        queued,
        failed,
        next_attempt_at,
        last_error,
    })
}

pub fn keychain_token(p: Provider) -> AppResult<Option<ProviderToken>> {
    secrets::get_json(p.as_str())
}

/// Validates `token` with `me()` and stores it as a `token` connection. A rejected token is
/// `invalidToken`.
pub async fn connect_token(app: &AppHandle, p: Provider, token: &str) -> AppResult<()> {
    backend::ensure_network(app)?;
    let token = token.trim();
    if token.is_empty() {
        return Err(AppError::Code("invalidToken"));
    }
    let label = me(p, token).await.map_err(|e| match e {
        ExportError::Unauthorized => AppError::Code("invalidToken"),
        other => other.into(),
    })?;
    secrets::set_json(
        p.as_str(),
        &ProviderToken {
            method: "token".into(),
            access_token: token.into(),
            refresh_token: None,
            expires_at: None,
            account_label: label,
            needs_reconnect: false,
        },
    )?;
    events::integration_changed(app, Some(p.as_str()), true, None);
    Ok(())
}

/// Fails this device's queued jobs of `p` with `disconnected`; the target row stays (it is
/// shared across devices). Returns the affected task ids.
pub fn fail_queued_on_disconnect(conn: &Connection, p: Provider, now: i64) -> AppResult<Vec<String>> {
    let device = device_id(conn)?;
    let mut stmt = conn.prepare(
        "UPDATE task_exports SET state = 'failed', error = 'disconnected', next_attempt_at = NULL, updated_at = ?3
         WHERE provider = ?1 AND device_id = ?2 AND state = 'queued' AND deleted_at IS NULL
         RETURNING task_id",
    )?;
    let ids = stmt.query_map(params![p.as_str(), device, now], |r| r.get(0))?;
    Ok(ids.collect::<Result<_, _>>()?)
}

/// Validates `config` against the provider's shape and upserts the live target row.
pub fn set_target(
    conn: &Connection,
    p: Provider,
    config: Value,
    target_label: &str,
    account_label: &str,
    now: i64,
) -> AppResult<ExportTarget> {
    let invalid = |_| AppError::Code("invalid");
    let config = match p {
        Provider::Notion => {
            let c: NotionConfig = serde_json::from_value(config).map_err(invalid)?;
            let status_ok = c.mapping.status.as_ref().is_none_or(|s| matches!(s.kind.as_str(), "status" | "select"));
            if c.data_source_id.trim().is_empty() || c.mapping.title.trim().is_empty() || !status_ok {
                return Err(AppError::Code("invalid"));
            }
            serde_json::to_value(c)?
        }
        Provider::Asana => {
            let c: AsanaConfig = serde_json::from_value(config).map_err(invalid)?;
            if c.workspace_gid.trim().is_empty() || c.project_gid.trim().is_empty() {
                return Err(AppError::Code("invalid"));
            }
            serde_json::to_value(c)?
        }
    };
    let target_label = target_label.trim();
    if target_label.is_empty() {
        return Err(AppError::Code("invalid"));
    }
    conn.execute(
        "INSERT INTO export_targets (id, provider, account_label, target_label, config, updated_at)
         VALUES (?1, ?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET account_label = excluded.account_label,
           target_label = excluded.target_label, config = excluded.config,
           updated_at = excluded.updated_at, deleted_at = NULL",
        params![p.as_str(), account_label, target_label, config.to_string(), now],
    )?;
    target(conn, p)?.ok_or(AppError::Code("notFound"))
}

fn job_state(conn: &Connection, id: &str) -> AppResult<Option<(TaskExportState, Option<i64>)>> {
    Ok(conn
        .query_row(
            "SELECT provider, state, error, external_url, next_attempt_at, deleted_at FROM task_exports WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    TaskExportState {
                        provider: r.get(0)?,
                        state: r.get(1)?,
                        error: r.get(2)?,
                        external_url: r.get(3)?,
                        next_attempt_at: r.get(4)?,
                    },
                    r.get(5)?,
                ))
            },
        )
        .optional()?)
}

/// Queues live task `task_id` for `p` on this device (`<task_id>:<provider>`). A live exported
/// job is `alreadyExported`; a queued/exporting one is returned as is; a failed or deleted one
/// is reset to `queued`.
pub fn enqueue(conn: &Connection, task_id: &str, p: Provider, now: i64) -> AppResult<TaskExportState> {
    let live: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1 AND deleted_at IS NULL)",
        [task_id],
        |r| r.get(0),
    )?;
    if !live {
        return Err(AppError::Code("notFound"));
    }
    let id = format!("{task_id}:{}", p.as_str());
    if let Some((s, None)) = job_state(conn, &id)? {
        match s.state.as_str() {
            "exported" => return Err(AppError::Code("alreadyExported")),
            "queued" | "exporting" => return Ok(s),
            _ => {}
        }
    }
    conn.execute(
        "INSERT INTO task_exports (id, task_id, provider, device_id, state, attempts, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 'queued', 0, ?5, ?5)
         ON CONFLICT(id) DO UPDATE SET device_id = excluded.device_id, state = 'queued', attempts = 0,
           next_attempt_at = NULL, error = NULL, external_id = NULL, external_url = NULL,
           updated_at = excluded.updated_at, deleted_at = NULL",
        params![id, task_id, p.as_str(), device_id(conn)?, now],
    )?;
    Ok(job_state(conn, &id)?.ok_or(AppError::Code("notFound"))?.0)
}

/// This device's `failed` and `queued` jobs (of `p`, or all) → `queued`, attempts 0, due now.
/// Returns the task ids.
pub fn retry(conn: &Connection, p: Option<Provider>, now: i64) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "UPDATE task_exports SET state = 'queued', attempts = 0, next_attempt_at = NULL, error = NULL, updated_at = ?3
         WHERE device_id = ?1 AND (?2 IS NULL OR provider = ?2) AND state IN ('failed','queued') AND deleted_at IS NULL
         RETURNING task_id",
    )?;
    let ids = stmt.query_map(params![device_id(conn)?, p.map(Provider::as_str), now], |r| r.get(0))?;
    Ok(ids.collect::<Result<_, _>>()?)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::db::{new_id, Db};

    pub(crate) fn task(conn: &Connection, title: &str) -> String {
        let id = new_id();
        conn.execute(
            "INSERT INTO tasks (id, title, notes, status, priority, created_at, updated_at)
             VALUES (?1, ?2, '', 'open', 0, 1, 1)",
            params![id, title],
        )
        .unwrap();
        id
    }

    #[test]
    fn source_url_rules() {
        let url_item = ItemRow { id: "i1".into(), kind: "url".into(), url: Some("https://a.b/c".into()), ..Default::default() };
        assert_eq!(source_url("t1", Some(&url_item)), "https://a.b/c");
        let file_item = ItemRow { id: "i2".into(), kind: "file".into(), url: Some("https://x".into()), ..Default::default() };
        assert_eq!(source_url("t1", Some(&file_item)), "copet://item/i2");
        let empty_url = ItemRow { id: "i3".into(), kind: "url".into(), ..Default::default() };
        assert_eq!(source_url("t1", Some(&empty_url)), "copet://item/i3");
        assert_eq!(source_url("t1", None), "copet://task/t1");
    }

    #[test]
    fn payload_tags_and_source() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        let id = task(&conn, "Write spec");
        let p = payload(&conn, Lang::Vi, Path::new("/lib"), &id).unwrap();
        assert_eq!(p.title, "Write spec");
        assert_eq!(p.tags, vec!["Việc cần làm".to_string()]);
        assert_eq!(p.source_url, format!("copet://task/{id}"));
        assert!(!p.done && p.file.is_none());
    }

    #[test]
    fn classify_maps_statuses() {
        assert_eq!(classify(429, None, String::new()), ExportError::RateLimited(30));
        assert_eq!(classify(529, Some("7"), String::new()), ExportError::RateLimited(7));
        assert_eq!(classify(401, None, String::new()), ExportError::Unauthorized);
        assert_eq!(classify(403, None, String::new()), ExportError::Unauthorized);
        assert_eq!(classify(404, None, String::new()), ExportError::TargetMissing);
        assert_eq!(classify(400, None, "bad".into()), ExportError::Invalid("bad".into()));
        assert_eq!(classify(502, None, String::new()), ExportError::Retryable);
    }

    #[test]
    fn retry_after_never_zero() {
        assert_eq!(classify(429, Some("0"), String::new()), ExportError::RateLimited(1));
        assert_eq!(classify(429, Some(" "), String::new()), ExportError::RateLimited(30));
        assert_eq!(classify(429, Some("-5"), String::new()), ExportError::RateLimited(30));
        assert_eq!(classify(429, Some("Wed, 21 Oct 2026 07:28:00 GMT"), String::new()), ExportError::RateLimited(30));
    }

    #[test]
    fn path_segment_percent_encodes() {
        assert_eq!(path_segment("1a2b-3c_d.e~f"), "1a2b-3c_d.e~f");
        assert_eq!(path_segment("a b+c/d?e#f"), "a%20b%2Bc%2Fd%3Fe%23f");
        assert_eq!(path_segment("ú"), "%C3%BA");
    }

    #[test]
    fn refresh_keeps_stored_refresh_token_when_null() {
        let old = ProviderToken {
            method: "oauth".into(),
            access_token: "a1".into(),
            refresh_token: Some("r1".into()),
            expires_at: Some(10),
            account_label: "me@x.y".into(),
            needs_reconnect: true,
        };
        assert!(needs_refresh(&old, 0));
        let t = merge_refreshed(
            old.clone(),
            BackendToken { access_token: "a2".into(), refresh_token: None, expires_at: Some(5_000_000) },
        );
        assert_eq!((t.access_token.as_str(), t.refresh_token.as_deref()), ("a2", Some("r1")));
        assert_eq!((t.expires_at, t.needs_reconnect, t.account_label.as_str()), (Some(5_000_000), false, "me@x.y"));
        assert!(!needs_refresh(&t, 0));
        let t = merge_refreshed(old, BackendToken { access_token: "a3".into(), refresh_token: Some("r2".into()), expires_at: None });
        assert_eq!(t.refresh_token.as_deref(), Some("r2"));
    }

    #[test]
    fn enqueue_rules() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        let id = task(&conn, "A");
        assert!(matches!(enqueue(&conn, "missing", Provider::Notion, 1), Err(AppError::Code("notFound"))));
        let s = enqueue(&conn, &id, Provider::Notion, 1).unwrap();
        assert_eq!((s.provider.as_str(), s.state.as_str()), ("notion", "queued"));
        conn.execute("UPDATE task_exports SET state = 'failed', error = 'network', attempts = 6", []).unwrap();
        let s = enqueue(&conn, &id, Provider::Notion, 2).unwrap();
        assert_eq!((s.state.as_str(), s.error), ("queued", None));
        conn.execute("UPDATE task_exports SET state = 'exported', external_url = 'https://n/1'", []).unwrap();
        assert!(matches!(enqueue(&conn, &id, Provider::Notion, 3), Err(AppError::Code("alreadyExported"))));
        let summary = tasks::get(&conn, &id).unwrap().task.export.unwrap();
        assert_eq!((summary.state.as_str(), summary.external_url.as_deref()), ("exported", Some("https://n/1")));
    }

    #[test]
    fn disconnect_fails_queued_and_keeps_target() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        let cfg = json!({ "workspaceGid": "w", "projectGid": "p", "sectionGid": null });
        set_target(&conn, Provider::Asana, cfg, "Co-pet › Sprint", "me@x.y", 1).unwrap();
        let a = task(&conn, "A");
        enqueue(&conn, &a, Provider::Asana, 1).unwrap();
        assert_eq!(fail_queued_on_disconnect(&conn, Provider::Asana, 2).unwrap(), vec![a.clone()]);
        let s = status(&conn, Provider::Asana, None).unwrap();
        assert_eq!((s.queued, s.failed, s.last_error.as_deref()), (0, 1, Some("disconnected")));
        assert_eq!(s.target.unwrap().target_label, "Co-pet › Sprint");
        assert_eq!(retry(&conn, Some(Provider::Asana), 3).unwrap(), vec![a]);
        assert!(set_target(&conn, Provider::Asana, json!({ "workspaceGid": "" , "projectGid": "p" }), "x", "y", 4).is_err());
    }
}
