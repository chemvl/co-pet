//! Background AI provider: the local model or the user's own cloud provider (Anthropic, OpenAI,
//! Gemini) or OpenAI-compatible server (Ollama, LM Studio). One provider serves triage, What's
//! Next and meeting summaries; [`resolve`] applies the routing rules.
pub mod schema;
pub mod wire;

use std::time::Duration;

use rusqlite::Connection;
use serde_json::Value;

use crate::db::settings;
use crate::error::{AppError, AppResult};

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
}
