//! The user's own Supabase project (entered in Settings › Account and kept in the device-only
//! settings `backend.supabaseUrl` / `backend.supabaseKey`), one shared HTTP client for every
//! network call, and the `privacy.localOnly` gate in front of them.
use std::sync::LazyLock;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rusqlite::Connection;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::db::{now_ms, settings, Db};
use crate::error::{AppError, AppResult};
use crate::export::{self, Provider};
use crate::model::BackendStatus;
use crate::{events, secrets};

pub const URL_KEY: &str = "backend.supabaseUrl";
pub const KEY_KEY: &str = "backend.supabaseKey";

#[derive(Clone)]
pub struct Backend {
    pub url: String,
    pub key: String,
}

/// The configured project; `None` until both values are set. `url` has no trailing `/`.
pub fn config(conn: &Connection) -> AppResult<Option<Backend>> {
    let url: String = settings::get(conn, URL_KEY)?;
    let key: String = settings::get(conn, KEY_KEY)?;
    Ok(parse(&url, &key))
}

/// [`config`] through the managed `Db`.
pub fn current(app: &AppHandle) -> AppResult<Option<Backend>> {
    config(&app.state::<Db>().conn())
}

fn parse(url: &str, key: &str) -> Option<Backend> {
    let url = url.trim().trim_end_matches('/');
    let key = key.trim();
    (!url.is_empty() && !key.is_empty()).then(|| Backend { url: url.to_string(), key: key.to_string() })
}

pub fn status(app: &AppHandle) -> AppResult<BackendStatus> {
    let b = current(app)?;
    Ok(BackendStatus { configured: b.is_some(), url: b.map(|b| b.url) })
}

/// The typed project, normalised: `invalidUrl` unless `https://` (plain `http://` only for a
/// local stack on `localhost`/`127.0.0.1`), `invalidKey` when empty, `secretKey` for a key
/// that must not leave the server.
fn validate(url: &str, key: &str) -> AppResult<Backend> {
    let (url, key) = (url.trim().trim_end_matches('/'), key.trim());
    if url.is_empty() {
        return Err(AppError::Code("invalidUrl"));
    }
    let b = parse(url, key).ok_or(AppError::Code("invalidKey"))?;
    let parsed = reqwest::Url::parse(&b.url).map_err(|_| AppError::Code("invalidUrl"))?;
    let local = matches!(parsed.host_str(), Some("localhost" | "127.0.0.1"));
    let scheme_ok = parsed.scheme() == "https" || (parsed.scheme() == "http" && local);
    if !scheme_ok || parsed.host_str().is_none_or(str::is_empty) || parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(AppError::Code("invalidUrl"));
    }
    if is_secret_key(&b.key) {
        return Err(AppError::Code("secretKey"));
    }
    Ok(b)
}

/// A key that bypasses row-level security: `sb_secret_…`, or a legacy JWT whose `role` is
/// `service_role`. It must never be stored on a device.
fn is_secret_key(key: &str) -> bool {
    if key.starts_with("sb_secret_") {
        return true;
    }
    let mut parts = key.split('.');
    let (Some(_), Some(payload), Some(_), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .is_some_and(|claims| claims["role"] == "service_role")
}

/// `GET {url}/auth/v1/settings` with the key: `backendUnreachable` when no answer arrives,
/// `backendRejected` when the project refuses the key or isn't a Supabase project.
async fn probe(b: &Backend) -> AppResult<()> {
    let res = http()
        .get(format!("{}/auth/v1/settings", b.url))
        .header("apikey", &b.key)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| {
            eprintln!("[backend] {e}");
            AppError::Code("backendUnreachable")
        })?;
    if !res.status().is_success() {
        eprintln!("[backend] {} answered {}", b.url, res.status());
        return Err(AppError::Code("backendRejected"));
    }
    Ok(())
}

/// Checks and tests the project, then saves it. Moving to another project first leaves the
/// old one: signs out, forgets this device's sync progress and the Notion/Asana connections
/// made through the old project's `oauth` function. The same project only replaces the key.
/// The first project also signs out a session an older build left behind.
pub async fn set(app: &AppHandle, url: &str, key: &str) -> AppResult<()> {
    ensure_network(app)?;
    let b = validate(url, key)?;
    probe(&b).await?;
    match current(app)? {
        Some(old) if old.url == b.url => {}
        Some(_) => {
            leave(app).await?;
            forget_oauth(app)?;
        }
        None => leave(app).await?,
    }
    save(app, &b.url, &b.key)?;
    changed(app);
    Ok(())
}

/// Forgets the project: signs out, forgets sync progress and OAuth-connected Notion/Asana.
pub async fn clear(app: &AppHandle) -> AppResult<()> {
    leave(app).await?;
    forget_oauth(app)?;
    save(app, "", "")?;
    changed(app);
    Ok(())
}

fn save(app: &AppHandle, url: &str, key: &str) -> AppResult<()> {
    settings::set(app, KEY_KEY, &json!(key))?;
    settings::set(app, URL_KEY, &json!(url))
}

/// Signs out and forgets this device's sync progress. `sync.userId` is cleared too: user ids
/// belong to one project, so keeping it would refuse every sign-in to the next (`otherAccount`).
/// This device's data stays.
async fn leave(app: &AppHandle) -> AppResult<()> {
    crate::account::auth::forget_session(app).await?;
    settings::set(app, "sync.userId", &Value::Null)?;
    crate::sync::forget(app).await
}

/// Disconnects Notion/Asana connected through the project's `oauth` function (their refresh
/// goes through it); personal tokens stay. Like the Disconnect button, queued jobs fail.
fn forget_oauth(app: &AppHandle) -> AppResult<()> {
    let mut failed = Vec::new();
    for p in Provider::ALL {
        if export::keychain_token(p)?.is_some_and(|t| t.method == "oauth") {
            secrets::delete(p.as_str())?;
            failed.extend(export::fail_queued_on_disconnect(&app.state::<Db>().conn(), p, now_ms())?);
        }
    }
    events::exports_changed(app, failed);
    Ok(())
}

fn changed(app: &AppHandle) {
    events::account_changed(app, None);
    crate::sync::poke(app);
}

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(concat!("Co-pet/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .expect("static reqwest client")
});

pub fn http() -> &'static reqwest::Client {
    &HTTP
}

/// `localOnly` while `privacy.localOnly` is on.
pub fn ensure_network(app: &AppHandle) -> AppResult<()> {
    if settings::read::<bool>(app, "privacy.localOnly")? {
        return Err(AppError::Code("localOnly"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code<T>(r: AppResult<T>) -> &'static str {
        match r {
            Err(AppError::Code(c)) => c,
            Err(e) => panic!("expected an error code, got {e}"),
            Ok(_) => panic!("expected an error code, got Ok"),
        }
    }

    #[test]
    fn config_needs_both_values() {
        let b = parse(" https://x.supabase.co/ ", "sb_publishable_1").unwrap();
        assert_eq!((b.url.as_str(), b.key.as_str()), ("https://x.supabase.co", "sb_publishable_1"));
        assert!(parse("https://x.supabase.co", "").is_none());
        assert!(parse("", "k").is_none());
        assert!(parse("https://x.supabase.co", " ").is_none());
    }

    #[test]
    fn config_reads_the_settings() {
        let db = Db::open_in_memory().unwrap();
        let c = db.conn();
        assert!(config(&c).unwrap().is_none(), "a new install is local-only");
        settings::put(&c, URL_KEY, &json!("https://x.supabase.co/")).unwrap();
        assert!(config(&c).unwrap().is_none());
        settings::put(&c, KEY_KEY, &json!("sb_publishable_1")).unwrap();
        let b = config(&c).unwrap().unwrap();
        assert_eq!((b.url.as_str(), b.key.as_str()), ("https://x.supabase.co", "sb_publishable_1"));
    }

    #[test]
    fn validate_wants_https_and_a_public_key() {
        let b = validate("https://abc.supabase.co/", " sb_publishable_x ").unwrap();
        assert_eq!((b.url.as_str(), b.key.as_str()), ("https://abc.supabase.co", "sb_publishable_x"));
        assert!(validate("http://127.0.0.1:54321", "k").is_ok());
        assert!(validate("http://localhost:54321", "k").is_ok());
        assert_eq!(code(validate("http://abc.supabase.co", "k")), "invalidUrl");
        assert_eq!(code(validate("abc.supabase.co", "k")), "invalidUrl");
        assert_eq!(code(validate("https://abc.supabase.co?x=1", "k")), "invalidUrl");
        assert_eq!(code(validate("", "k")), "invalidUrl");
        assert_eq!(code(validate("https://abc.supabase.co", " ")), "invalidKey");
        assert_eq!(code(validate("https://abc.supabase.co", "sb_secret_abc")), "secretKey");
        let jwt = |role: &str| {
            let claims = URL_SAFE_NO_PAD.encode(format!(r#"{{"iss":"supabase","role":"{role}"}}"#));
            format!("eyJhbGciOiJIUzI1NiJ9.{claims}.sig")
        };
        assert_eq!(code(validate("https://abc.supabase.co", &jwt("service_role"))), "secretKey");
        assert!(validate("https://abc.supabase.co", &jwt("anon")).is_ok());
    }
}
