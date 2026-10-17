//! Background AI provider: the local model or the user's own cloud provider (Anthropic, OpenAI,
//! Gemini) or OpenAI-compatible server (Ollama, LM Studio). One provider serves triage, What's
//! Next and meeting summaries; [`resolve`] applies the routing rules.
pub mod schema;
pub mod wire;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use rusqlite::Connection;
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Manager};

use crate::db::{settings, Db};
use crate::error::{AppError, AppResult};
use crate::llm::download::Downloader;
use crate::llm::LlmServer;
use crate::model::{CloudModelInfo, CloudProviderStatus, CloudStatus};
use crate::{backend, events, secrets};

/// Per-call options for a structured-output chat: the `json_schema` name, the reply token limit
/// and the request timeout.
pub struct ChatOpts {
    pub name: &'static str,
    pub max_tokens: u32,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Local,
    Anthropic,
    OpenAi,
    Gemini,
    Ollama,
    LmStudio,
}

impl Provider {
    pub const CLOUD: [Provider; 5] =
        [Provider::Anthropic, Provider::OpenAi, Provider::Gemini, Provider::Ollama, Provider::LmStudio];

    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Local => "local",
            Provider::Anthropic => "anthropic",
            Provider::OpenAi => "openai",
            Provider::Gemini => "gemini",
            Provider::Ollama => "ollama",
            Provider::LmStudio => "lmstudio",
        }
    }

    /// Unknown names → `invalid`.
    pub fn parse(s: &str) -> AppResult<Provider> {
        std::iter::once(Provider::Local)
            .chain(Provider::CLOUD)
            .find(|p| p.as_str() == s)
            .ok_or(AppError::Code("invalid"))
    }

    pub fn needs_key(self) -> bool {
        matches!(self, Provider::Anthropic | Provider::OpenAi | Provider::Gemini)
    }

    /// An OpenAI-compatible server at a user-given address.
    pub fn is_server(self) -> bool {
        matches!(self, Provider::Ollama | Provider::LmStudio)
    }

    /// Keychain account (service `com.copet.app`) holding this provider's API key.
    pub fn keychain_account(self) -> Option<&'static str> {
        match self {
            Provider::Anthropic => Some("ai.anthropic"),
            Provider::OpenAi => Some("ai.openai"),
            Provider::Gemini => Some("ai.gemini"),
            _ => None,
        }
    }
}

pub const KEY_ACCOUNTS: [&str; 3] = ["ai.anthropic", "ai.openai", "ai.gemini"];

/// A curated cloud model; `effort` is the reasoning effort sent with it ("" = omit the parameter).
pub struct CloudModel {
    pub provider: Provider,
    pub id: &'static str,
    pub label: &'static str,
    pub effort: &'static str,
}

/// Per provider, the first entry is the default.
pub const CLOUD_MODELS: [CloudModel; 6] = [
    CloudModel { provider: Provider::Anthropic, id: "claude-haiku-5-5", label: "Claude Haiku 5.5", effort: "low" },
    CloudModel { provider: Provider::Anthropic, id: "claude-sonnet-5-5", label: "Claude Sonnet 5.5", effort: "low" },
    CloudModel { provider: Provider::OpenAi, id: "gpt-6-luna", label: "GPT-6 Luna", effort: "none" },
    CloudModel { provider: Provider::OpenAi, id: "gpt-6.1-sol", label: "GPT-6.1 Sol", effort: "low" },
    CloudModel { provider: Provider::Gemini, id: "gemini-3.5-flash-lite", label: "Gemini 3.5 Flash-Lite", effort: "minimal" },
    CloudModel { provider: Provider::Gemini, id: "gemini-3.8-flash", label: "Gemini 3.8 Flash", effort: "low" },
];

fn curated(p: Provider, id: &str) -> Option<&'static CloudModel> {
    CLOUD_MODELS.iter().find(|m| m.provider == p && m.id == id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Triage,
    #[allow(dead_code)] // What's Next moves onto `ai::chat_json` in Task 4.3
    Next,
    Meeting,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AiConfig {
    pub provider: Provider,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub cloud_meetings: bool,
    pub local_only: bool,
}

impl AiConfig {
    /// `provider` None = the `ai.provider` setting. Curated providers: the stored model when it
    /// is in CLOUD_MODELS, else that provider's first entry. Servers: the stored non-empty model
    /// or None; `base_url` from `ai.serverUrls`.
    pub fn read(conn: &Connection, provider: Option<Provider>) -> AppResult<AiConfig> {
        let provider = match provider {
            Some(p) => p,
            None => Provider::parse(&settings::get::<String>(conn, "ai.provider")?)?,
        };
        let name = provider.as_str();
        let (model, base_url) = if provider.needs_key() {
            let models: Value = settings::get(conn, "ai.cloudModels")?;
            let stored = models[name].as_str().and_then(|id| curated(provider, id));
            let model = stored.or_else(|| CLOUD_MODELS.iter().find(|m| m.provider == provider));
            (model.map(|m| m.id.to_owned()), None)
        } else if provider.is_server() {
            let models: Value = settings::get(conn, "ai.cloudModels")?;
            let urls: Value = settings::get(conn, "ai.serverUrls")?;
            let model = models[name].as_str().map(str::trim).filter(|m| !m.is_empty()).map(str::to_owned);
            let url = urls[name].as_str().filter(|u| !u.is_empty()).map(str::to_owned);
            (model, url)
        } else {
            (None, None)
        };
        Ok(AiConfig {
            provider,
            model,
            base_url,
            cloud_meetings: settings::get(conn, "ai.cloudMeetings")?,
            local_only: settings::get(conn, "privacy.localOnly")?,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Route {
    Local,
    Cloud { provider: Provider, model: String, effort: Option<&'static str>, base_url: Option<String> },
}

impl Route {
    #[allow(dead_code)] // triage's Why provider reads it from Task 4.3
    pub fn provider(&self) -> Provider {
        match self {
            Route::Local => Provider::Local,
            Route::Cloud { provider, .. } => *provider,
        }
    }
}

/// The routing rules, in order:
/// 1. provider `local` → the local model if downloaded, else `noModel`;
/// 2. a meeting summary without `ai.cloudMeetings` → the local model if downloaded, else
///    `cloudMeetingsOff`;
/// 3. `privacy.localOnly` → `localOnly`;
/// 4. a key provider without a key, or a cloud provider without a model (or a server without an
///    address) → `notSetUp`;
/// 5. otherwise the cloud call.
pub fn resolve(c: &AiConfig, purpose: Purpose, local_ready: bool, has_key: bool) -> Result<Route, &'static str> {
    let local = |err: &'static str| if local_ready { Ok(Route::Local) } else { Err(err) };
    if c.provider == Provider::Local {
        return local("noModel");
    }
    if purpose == Purpose::Meeting && !c.cloud_meetings {
        return local("cloudMeetingsOff");
    }
    if c.local_only {
        return Err("localOnly");
    }
    if c.provider.needs_key() && !has_key {
        return Err("notSetUp");
    }
    let Some(model) = c.model.clone() else { return Err("notSetUp") };
    if c.provider.is_server() && c.base_url.is_none() {
        return Err("notSetUp");
    }
    let effort = curated(c.provider, &model).map(|m| m.effort).filter(|e| !e.is_empty());
    Ok(Route::Cloud { provider: c.provider, model, effort, base_url: c.base_url.clone() })
}

/// http(s) origin of an Ollama / LM Studio server; path "", "/", "/v1" or "/v1/" is accepted
/// and dropped; query, fragment, credentials or any other path → `invalid`.
pub fn normalize_url(s: &str) -> AppResult<String> {
    let u = url::Url::parse(s.trim()).map_err(|_| AppError::Code("invalid"))?;
    let ok = matches!(u.scheme(), "http" | "https")
        && u.host().is_some()
        && u.username().is_empty()
        && u.password().is_none()
        && u.query().is_none()
        && u.fragment().is_none()
        && matches!(u.path(), "" | "/" | "/v1" | "/v1/");
    if !ok {
        return Err(AppError::Code("invalid"));
    }
    Ok(u.origin().ascii_serialization())
}

/// Managed (`app.manage(ai::KeyCache::default())` right after `app.manage(db)` in lib.rs setup):
/// whether each key account holds a key, read from the keychain once per provider.
#[derive(Default)]
pub struct KeyCache(Mutex<HashMap<&'static str, bool>>);

impl KeyCache {
    /// False for providers without a key account. A keychain read error is logged, reads as
    /// false and is not cached.
    pub fn has(&self, p: Provider) -> bool {
        let Some(account) = p.keychain_account() else { return false };
        let mut known = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(&present) = known.get(account) {
            return present;
        }
        match secrets::get(account) {
            Ok(key) => *known.entry(account).or_insert(key.is_some()),
            Err(e) => {
                eprintln!("[ai] {e}");
                false
            }
        }
    }

    pub fn set(&self, p: Provider, present: bool) {
        if let Some(account) = p.keychain_account() {
            self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(account, present);
        }
    }
}

/// [`resolve`] over the saved settings, the downloaded local model and the cached key state.
pub fn route(app: &AppHandle, purpose: Purpose) -> Result<Route, &'static str> {
    let config = AiConfig::read(&app.state::<Db>().conn(), None).map_err(|e| {
        eprintln!("[ai] settings: {e}");
        "notSetUp"
    })?;
    let local_ready = crate::llm::active_model_path(app).is_some();
    let has_key = app.state::<KeyCache>().has(config.provider);
    resolve(&config, purpose, local_ready, has_key)
}

/// Whether a model call for `purpose` would go out now (see [`route`]).
#[allow(dead_code)] // triage, What's Next and meeting summaries call it from Task 4.3
pub fn ready(app: &AppHandle, purpose: Purpose) -> bool {
    route(app, purpose).is_ok()
}

/// The single entry point for every model call: the local `llama-server` or the routed cloud
/// provider. Errors: the [`resolve`] codes, then those of the call itself.
#[allow(dead_code)] // triage, What's Next and meeting summaries call it from Task 4.3
pub async fn chat_json(
    app: &AppHandle,
    purpose: Purpose,
    opts: &ChatOpts,
    system: &str,
    user: &str,
    schema: &Value,
) -> AppResult<Value> {
    let route = route(app, purpose).map_err(AppError::Code)?;
    call(app, &route, opts, system, user, schema).await
}

/// One structured-output call over `route`. Cloud errors: `notSetUp` (key gone from the
/// keychain), `transport_code` when no reply came, `status_code` for a non-2xx reply.
async fn call(app: &AppHandle, route: &Route, opts: &ChatOpts, system: &str, user: &str, schema: &Value) -> AppResult<Value> {
    let Route::Cloud { provider, model, effort, base_url } = route else {
        let llm = app.state::<LlmServer>();
        llm.ensure_running(app).await?;
        return llm.chat_json(opts, system, user, schema).await;
    };
    let p = *provider;
    let key = match p.keychain_account() {
        Some(account) => match secrets::get(account)? {
            Some(key) => Some(key),
            None => {
                app.state::<KeyCache>().set(p, false);
                return Err(AppError::Code("notSetUp"));
            }
        },
        None => None,
    };
    let target =
        wire::Target { provider: p, model, effort: *effort, base_url: base_url.as_deref(), key: key.as_deref() };
    let req = wire::request(&target, opts, system, user, &schema::for_provider(schema, p));
    let mut http = backend::http().post(&req.url).json(&req.body).timeout(opts.timeout);
    for (name, value) in &req.headers {
        http = http.header(*name, value);
    }
    let resp = http.send().await.map_err(|e| {
        eprintln!("[ai] {}: {}", p.as_str(), e.without_url());
        AppError::Code(wire::transport_code(p))
    })?;
    let status = resp.status().as_u16();
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        eprintln!("[ai] {} {status}: {}", p.as_str(), body.chars().take(300).collect::<String>());
        return Err(AppError::Code(wire::status_code(p, status, &body)));
    }
    let reply: Value = resp.json().await.map_err(|e| {
        eprintln!("[ai] {} reply: {}", p.as_str(), e.without_url());
        AppError::Code(wire::transport_code(p))
    })?;
    let mut v: Value = serde_json::from_str(&wire::reply_text(p, &reply)?)?;
    schema::clamp(&mut v, schema);
    Ok(v)
}

/// Sets `setting[p]` in a JSON object setting (`ai.cloudModels`, `ai.serverUrls`).
fn set_entry(app: &AppHandle, setting: &str, p: Provider, value: &str) -> AppResult<()> {
    let mut map: Map<String, Value> = settings::read(app, setting)?;
    map.insert(p.as_str().to_owned(), json!(value));
    settings::set(app, setting, &Value::Object(map))
}

fn emit_status(app: &AppHandle) {
    events::ai_status(app, app.state::<Downloader>().status(app));
}

/// `configure`'s model and address for `p`: a curated id for key providers, a 1–200 character
/// trimmed name for servers, none for local; an address for servers only. Else `invalid`.
fn configure_args<'a>(p: Provider, model: Option<&'a str>, base_url: Option<&str>) -> AppResult<(Option<&'a str>, Option<String>)> {
    let model = match model {
        None => None,
        Some(m) if p.needs_key() => Some(curated(p, m).ok_or(AppError::Code("invalid"))?.id),
        Some(m) if p.is_server() && (1..=200).contains(&m.trim().chars().count()) => Some(m.trim()),
        Some(_) => return Err(AppError::Code("invalid")),
    };
    let base_url = match base_url {
        None => None,
        Some(u) if p.is_server() => Some(normalize_url(u)?),
        Some(_) => return Err(AppError::Code("invalid")),
    };
    Ok((model, base_url))
}

/// Makes `provider` the active one and saves its model (curated id, or a server's model name)
/// and server address. Errors: `invalid`, `localOnly` (any non-local provider).
pub async fn configure(app: &AppHandle, provider: &str, model: Option<&str>, base_url: Option<&str>) -> AppResult<()> {
    let p = Provider::parse(provider)?;
    if p != Provider::Local {
        backend::ensure_network(app)?;
    }
    let (model, base_url) = configure_args(p, model, base_url)?;
    let previous: String = settings::read(app, "ai.provider")?;
    if let Some(m) = model {
        set_entry(app, "ai.cloudModels", p, m)?;
    }
    if let Some(u) = &base_url {
        set_entry(app, "ai.serverUrls", p, u)?;
    }
    settings::set(app, "ai.provider", &json!(p.as_str()))?;
    if previous == Provider::Local.as_str() && p != Provider::Local {
        app.state::<LlmServer>().stop().await;
    }
    crate::transcribe::worker::summaries_unblocked(app);
    emit_status(app);
    Ok(())
}

/// The trimmed key when it is 8–512 characters without whitespace, else `invalid`.
fn checked_key(key: &str) -> AppResult<&str> {
    let key = key.trim();
    if !(8..=512).contains(&key.chars().count()) || key.chars().any(char::is_whitespace) {
        return Err(AppError::Code("invalid"));
    }
    Ok(key)
}

/// Saves a key provider's API key in the keychain. Error `invalid` (not a key provider, or a
/// malformed key).
pub fn set_key(app: &AppHandle, provider: &str, key: &str) -> AppResult<()> {
    let p = Provider::parse(provider)?;
    let account = p.keychain_account().ok_or(AppError::Code("invalid"))?;
    let key = checked_key(key)?;
    secrets::set(account, key)?;
    app.state::<KeyCache>().set(p, true);
    crate::transcribe::worker::summaries_unblocked(app);
    emit_status(app);
    Ok(())
}

/// Error `invalid` (not a key provider).
pub fn remove_key(app: &AppHandle, provider: &str) -> AppResult<()> {
    let p = Provider::parse(provider)?;
    let account = p.keychain_account().ok_or(AppError::Code("invalid"))?;
    secrets::delete(account)?;
    app.state::<KeyCache>().set(p, false);
    emit_status(app);
    Ok(())
}

const TEST_CHAT: ChatOpts = ChatOpts { name: "test", max_tokens: 16, timeout: Duration::from_secs(20) };

/// One tiny call with `provider`'s saved setup (not necessarily the active provider). Errors:
/// `localOnly`, `invalid` (local), `notSetUp`, the call's codes, `aiRequest` (no boolean `ok`).
pub async fn test(app: &AppHandle, provider: &str) -> AppResult<()> {
    let p = Provider::parse(provider)?;
    backend::ensure_network(app)?;
    if p == Provider::Local {
        return Err(AppError::Code("invalid"));
    }
    let config = AiConfig::read(&app.state::<Db>().conn(), Some(p))?;
    let has_key = app.state::<KeyCache>().has(p);
    let route = resolve(&config, Purpose::Triage, false, has_key).map_err(AppError::Code)?;
    let schema = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["ok"],
        "properties": { "ok": { "type": "boolean" } },
    });
    let reply = call(app, &route, &TEST_CHAT, "Answer with ok set to true.", "ping", &schema).await?;
    if !reply["ok"].is_boolean() {
        return Err(AppError::Code("aiRequest"));
    }
    Ok(())
}

/// The model ids an Ollama / LM Studio server offers (`GET <base>/v1/models`), sorted. Errors:
/// `invalid` (not a server provider), `localOnly`, `notSetUp` (no address), `serverUnreachable`,
/// `status_code` for a non-2xx reply.
pub async fn server_models(app: &AppHandle, provider: &str) -> AppResult<Vec<String>> {
    let p = Provider::parse(provider)?;
    if !p.is_server() {
        return Err(AppError::Code("invalid"));
    }
    backend::ensure_network(app)?;
    let base = AiConfig::read(&app.state::<Db>().conn(), Some(p))?.base_url.ok_or(AppError::Code("notSetUp"))?;
    let resp = backend::http()
        .get(format!("{base}/v1/models"))
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| {
            eprintln!("[ai] {} models: {}", p.as_str(), e.without_url());
            AppError::Code("serverUnreachable")
        })?;
    let status = resp.status().as_u16();
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        eprintln!("[ai] {} models {status}: {}", p.as_str(), body.chars().take(300).collect::<String>());
        return Err(AppError::Code(wire::status_code(p, status, &body)));
    }
    Ok(model_ids(&resp.json().await?))
}

/// `data[].id` of an OpenAI-style model list, sorted.
fn model_ids(list: &Value) -> Vec<String> {
    let mut ids: Vec<String> = list["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["id"].as_str().map(str::to_owned))
        .collect();
    ids.sort();
    ids
}

/// The active provider and every cloud provider's saved setup, for `AiStatus.cloud`.
pub fn cloud_status(app: &AppHandle) -> CloudStatus {
    // Before the database lock: a first read per provider goes to the keychain.
    let keys = app.state::<KeyCache>();
    let has_key: Vec<bool> = Provider::CLOUD.iter().map(|&p| keys.has(p)).collect();
    let db = app.state::<Db>();
    let conn = db.conn();
    let provider = settings::get::<String>(&conn, "ai.provider").unwrap_or_else(|e| {
        eprintln!("[ai] settings: {e}");
        Provider::Local.as_str().to_owned()
    });
    let providers = Provider::CLOUD
        .iter()
        .zip(has_key)
        .map(|(&p, has_key)| provider_status(&conn, p, has_key))
        .collect();
    CloudStatus { provider, providers }
}

fn provider_status(conn: &Connection, p: Provider, has_key: bool) -> CloudProviderStatus {
    let (model, base_url) = match AiConfig::read(conn, Some(p)) {
        Ok(c) => (c.model, c.base_url),
        Err(e) => {
            eprintln!("[ai] settings: {e}");
            (None, None)
        }
    };
    let ready = model.is_some() && (!p.needs_key() || has_key) && (!p.is_server() || base_url.is_some());
    CloudProviderStatus {
        id: p.as_str().to_owned(),
        has_key,
        model,
        base_url,
        models: CLOUD_MODELS
            .iter()
            .filter(|m| m.provider == p)
            .map(|m| CloudModelInfo { id: m.id.to_owned(), label: m.label.to_owned() })
            .collect(),
        ready,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn cfg(provider: Provider) -> AiConfig {
        let model = match provider {
            Provider::Local => None,
            p if p.is_server() => Some("qwen3:4b".to_owned()),
            p => CLOUD_MODELS.iter().find(|m| m.provider == p).map(|m| m.id.to_owned()),
        };
        let base_url = provider.is_server().then(|| "http://localhost:11434".to_owned());
        AiConfig { provider, model, base_url, cloud_meetings: false, local_only: false }
    }

    fn code(r: AppResult<String>) -> &'static str {
        match r {
            Err(AppError::Code(c)) => c,
            other => panic!("expected a code, got {other:?}"),
        }
    }

    #[test]
    fn resolve_local_needs_downloaded_model() {
        let c = cfg(Provider::Local);
        for purpose in [Purpose::Triage, Purpose::Next, Purpose::Meeting] {
            assert_eq!(resolve(&c, purpose, true, false), Ok(Route::Local));
            assert_eq!(resolve(&c, purpose, false, true), Err("noModel"));
        }
        // Local-only does not affect the local model.
        let c = AiConfig { local_only: true, ..cfg(Provider::Local) };
        assert_eq!(resolve(&c, Purpose::Triage, true, false), Ok(Route::Local));
    }

    #[test]
    fn resolve_meetings_stay_local_without_opt_in() {
        for p in [Provider::Anthropic, Provider::Ollama] {
            let c = cfg(p);
            assert_eq!(resolve(&c, Purpose::Meeting, true, true), Ok(Route::Local));
            assert_eq!(resolve(&c, Purpose::Meeting, false, true), Err("cloudMeetingsOff"));
            assert_eq!(resolve(&c, Purpose::Triage, true, true).unwrap().provider(), p);
        }
        let c = AiConfig { cloud_meetings: true, ..cfg(Provider::OpenAi) };
        assert_eq!(resolve(&c, Purpose::Meeting, true, true).unwrap().provider(), Provider::OpenAi);
    }

    #[test]
    fn resolve_refuses_cloud_in_local_only() {
        let c = AiConfig { local_only: true, ..cfg(Provider::Gemini) };
        assert_eq!(resolve(&c, Purpose::Triage, true, true), Err("localOnly"));
        assert_eq!(resolve(&c, Purpose::Next, true, true), Err("localOnly"));
        let c = AiConfig { cloud_meetings: true, ..c };
        assert_eq!(resolve(&c, Purpose::Meeting, true, true), Err("localOnly"));
        let c = AiConfig { local_only: true, ..cfg(Provider::LmStudio) };
        assert_eq!(resolve(&c, Purpose::Triage, true, true), Err("localOnly"));
    }

    #[test]
    fn resolve_cloud_needs_key_and_model() {
        assert_eq!(resolve(&cfg(Provider::Anthropic), Purpose::Triage, true, false), Err("notSetUp"));
        let c = AiConfig { model: None, ..cfg(Provider::Ollama) };
        assert_eq!(resolve(&c, Purpose::Next, true, false), Err("notSetUp"));
        assert_eq!(
            resolve(&cfg(Provider::OpenAi), Purpose::Triage, false, true),
            Ok(Route::Cloud { provider: Provider::OpenAi, model: "gpt-6-luna".into(), effort: Some("none"), base_url: None })
        );
        let c = AiConfig { model: Some("gemini-3.8-flash".into()), ..cfg(Provider::Gemini) };
        assert_eq!(
            resolve(&c, Purpose::Next, false, true),
            Ok(Route::Cloud { provider: Provider::Gemini, model: "gemini-3.8-flash".into(), effort: Some("low"), base_url: None })
        );
        // Servers take no key and send no effort.
        assert_eq!(
            resolve(&cfg(Provider::Ollama), Purpose::Triage, false, false),
            Ok(Route::Cloud {
                provider: Provider::Ollama,
                model: "qwen3:4b".into(),
                effort: None,
                base_url: Some("http://localhost:11434".into()),
            })
        );
    }

    #[test]
    fn config_reads_seeded_defaults() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        let c = AiConfig::read(&conn, None).unwrap();
        assert_eq!(c.provider, Provider::Local);
        assert_eq!((c.model, c.base_url, c.cloud_meetings, c.local_only), (None, None, false, false));
        let a = AiConfig::read(&conn, Some(Provider::Anthropic)).unwrap();
        assert_eq!(a.model.as_deref(), Some("claude-haiku-5-5"));
        let o = AiConfig::read(&conn, Some(Provider::Ollama)).unwrap();
        assert_eq!((o.model, o.base_url.as_deref()), (None, Some("http://localhost:11434")));
        let l = AiConfig::read(&conn, Some(Provider::LmStudio)).unwrap();
        assert_eq!(l.base_url.as_deref(), Some("http://localhost:1234"));
        // A stored model outside the catalog falls back to the provider default.
        settings::put(&conn, "ai.cloudModels", &serde_json::json!({ "openai": "gpt-unknown", "ollama": " qwen3:4b " }))
            .unwrap();
        settings::put(&conn, "ai.provider", &serde_json::json!("openai")).unwrap();
        let c = AiConfig::read(&conn, None).unwrap();
        assert_eq!((c.provider, c.model.as_deref()), (Provider::OpenAi, Some("gpt-6-luna")));
        let o = AiConfig::read(&conn, Some(Provider::Ollama)).unwrap();
        assert_eq!(o.model.as_deref(), Some("qwen3:4b"));
    }

    #[test]
    fn server_url_validation() {
        assert_eq!(normalize_url("http://localhost:1234/v1/").unwrap(), "http://localhost:1234");
        assert_eq!(normalize_url("http://localhost:11434").unwrap(), "http://localhost:11434");
        assert_eq!(normalize_url("https://Box.local/v1").unwrap(), "https://box.local");
        for bad in ["ftp://x", "http://h/api", "http://h/?a", "http://h/#x", "http://u:p@h/", "localhost:11434", ""] {
            assert_eq!(code(normalize_url(bad)), "invalid", "{bad}");
        }
    }

    #[test]
    fn provider_names_round_trip() {
        for p in std::iter::once(Provider::Local).chain(Provider::CLOUD) {
            assert_eq!(Provider::parse(p.as_str()).unwrap(), p);
        }
        assert!(matches!(Provider::parse("claude"), Err(AppError::Code("invalid"))));
        let accounts: Vec<&str> = Provider::CLOUD.iter().filter_map(|p| p.keychain_account()).collect();
        assert_eq!(accounts, KEY_ACCOUNTS);
    }

    fn invalid<T: std::fmt::Debug>(r: AppResult<T>) -> bool {
        matches!(r, Err(AppError::Code("invalid")))
    }

    #[test]
    fn configure_args_validate_per_provider() {
        assert_eq!(configure_args(Provider::Local, None, None).unwrap(), (None, None));
        assert!(invalid(configure_args(Provider::Local, Some("x"), None)));
        assert!(invalid(configure_args(Provider::Local, None, Some("http://localhost:1234"))));
        assert_eq!(configure_args(Provider::Anthropic, Some("claude-sonnet-5-5"), None).unwrap().0, Some("claude-sonnet-5-5"));
        assert!(invalid(configure_args(Provider::Anthropic, Some("gpt-6-luna"), None)));
        assert!(invalid(configure_args(Provider::OpenAi, None, Some("http://localhost:1234"))));
        assert_eq!(
            configure_args(Provider::Ollama, Some(" qwen3:4b "), Some("http://localhost:11434/v1")).unwrap(),
            (Some("qwen3:4b"), Some("http://localhost:11434".to_owned()))
        );
        assert!(invalid(configure_args(Provider::LmStudio, Some("  "), None)));
        assert!(invalid(configure_args(Provider::LmStudio, Some(&"m".repeat(201)), None)));
        assert!(configure_args(Provider::LmStudio, Some(&"m".repeat(200)), None).is_ok());
        assert!(invalid(configure_args(Provider::Ollama, None, Some("http://h/api"))));
    }

    #[test]
    fn keys_are_trimmed_and_bounded() {
        assert_eq!(checked_key("  sk-ant-12345  ").unwrap(), "sk-ant-12345");
        assert!(invalid(checked_key("1234567")));
        assert!(checked_key("12345678").is_ok());
        assert!(invalid(checked_key(&"k".repeat(513))));
        assert!(invalid(checked_key("sk-ant 12345")));
    }

    #[test]
    fn key_cache_remembers_set_values() {
        let cache = KeyCache::default();
        cache.set(Provider::Anthropic, true);
        assert!(cache.has(Provider::Anthropic));
        cache.set(Provider::Anthropic, false);
        assert!(!cache.has(Provider::Anthropic));
        // Servers have no key account, so nothing is read or stored.
        cache.set(Provider::Ollama, true);
        assert!(!cache.has(Provider::Ollama));
    }

    #[test]
    fn provider_status_reports_setup() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        let a = provider_status(&conn, Provider::Anthropic, false);
        assert_eq!((a.id.as_str(), a.model.as_deref(), a.ready), ("anthropic", Some("claude-haiku-5-5"), false));
        let labels: Vec<&str> = a.models.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(labels, ["Claude Haiku 5.5", "Claude Sonnet 5.5"]);
        assert!(provider_status(&conn, Provider::Anthropic, true).ready);
        let o = provider_status(&conn, Provider::Ollama, false);
        assert_eq!((o.model, o.base_url.as_deref(), o.ready), (None, Some("http://localhost:11434"), false));
        assert!(o.models.is_empty());
        settings::put(&conn, "ai.cloudModels", &json!({ "ollama": "qwen3:4b" })).unwrap();
        assert!(provider_status(&conn, Provider::Ollama, false).ready);
    }

    #[test]
    fn server_model_ids_are_sorted() {
        let list = json!({ "object": "list", "data": [{ "id": "qwen3:4b" }, { "id": "llama3.2" }, { "object": "model" }] });
        assert_eq!(model_ids(&list), ["llama3.2", "qwen3:4b"]);
        assert!(model_ids(&json!({})).is_empty());
    }
}
