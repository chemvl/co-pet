//! Notion/Asana OAuth through the backend `oauth` function (PKCE). `start` opens the browser;
//! the function redirects to `copet://oauth/done?state=…`, and `finish` claims the token with
//! the verifier only this process knows.
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::json;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;
use url::Url;

use super::{me, BackendToken, Provider};
use crate::backend;
use crate::db::settings;
use crate::error::{AppError, AppResult};
use crate::events;
use crate::secrets::{self, ProviderToken};
use crate::windows;

/// The backend parks a handoff for 10 minutes.
const TTL: Duration = Duration::from_secs(10 * 60);

/// Handoffs this process started. `finished` keeps every state already taken, so a deep link
/// delivered twice (cold launch) is ignored instead of reported as expired.
#[derive(Default)]
pub struct OAuthPending(Mutex<Handoffs>);

#[derive(Default)]
struct Handoffs {
    pending: HashMap<String, (Provider, String, Instant)>,
    finished: HashSet<String>,
}

#[derive(Debug, PartialEq)]
enum Claim {
    /// Already handled.
    Ignore,
    /// Never issued here (`None`) or older than 10 minutes.
    Expired(Option<Provider>),
    Go(Provider, String),
}

impl Handoffs {
    fn insert(&mut self, state: String, p: Provider, verifier: String, now: Instant) {
        self.pending.retain(|_, (_, _, at)| now.duration_since(*at) <= TTL);
        self.pending.insert(state, (p, verifier, now));
    }

    fn take(&mut self, state: &str, now: Instant) -> Claim {
        match self.pending.remove(state) {
            Some((p, verifier, at)) => {
                self.finished.insert(state.to_string());
                if now.duration_since(at) > TTL {
                    Claim::Expired(Some(p))
                } else {
                    Claim::Go(p, verifier)
                }
            }
            None if self.finished.contains(state) => Claim::Ignore,
            None => Claim::Expired(None),
        }
    }
}

impl OAuthPending {
    fn lock(&self) -> std::sync::MutexGuard<'_, Handoffs> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// 32 random bytes, base64url without padding (43 characters).
pub(crate) fn random_b64() -> AppResult<String> {
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf).map_err(|e| AppError::msg(format!("random: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// PKCE `S256`: base64url (no padding) of the verifier's SHA-256.
pub(crate) fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn start_url(base: &str, p: Provider, state: &str, challenge: &str) -> AppResult<Url> {
    let mut url = Url::parse(&format!("{base}/functions/v1/oauth/start")).map_err(|e| AppError::msg(e.to_string()))?;
    url.query_pairs_mut()
        .append_pair("provider", p.as_str())
        .append_pair("state", state)
        .append_pair("challenge", challenge);
    Ok(url)
}

/// Opens the provider's consent page in the browser. `localOnly`, `notConfigured`.
pub fn start(app: &AppHandle, p: Provider) -> AppResult<()> {
    backend::ensure_network(app)?;
    let b = backend::config().ok_or(AppError::Code("notConfigured"))?;
    let state = random_b64()?;
    let verifier = random_b64()?;
    let url = start_url(b.url, p, &state, &challenge(&verifier))?;
    app.state::<OAuthPending>().lock().insert(state, p, verifier, Instant::now());
    app.opener().open_url(url.as_str(), None::<&str>).map_err(|e| AppError::msg(e.to_string()))
}

/// The `copet://oauth/done` deep link. Emits `integration-changed`; on success opens Home's
/// Integrations, unless onboarding is still running (its cards listen for the event).
pub async fn finish(app: AppHandle, state: String, error: Option<String>) {
    let claim = app.state::<OAuthPending>().lock().take(&state, Instant::now());
    let (p, verifier) = match claim {
        Claim::Ignore => return,
        Claim::Expired(p) => {
            events::integration_changed(&app, p.map(Provider::as_str), false, Some("oauthExpired"));
            return;
        }
        Claim::Go(p, verifier) => (p, verifier),
    };
    if error.is_some() {
        events::integration_changed(&app, Some(p.as_str()), false, Some("oauthDenied"));
        return;
    }
    match claim_token(&app, p, &state, &verifier).await {
        Ok(()) => {
            events::integration_changed(&app, Some(p.as_str()), true, None);
            if settings::read::<bool>(&app, "onboarding.completed").unwrap_or(false) {
                if let Err(e) = windows::open_home(&app, Some("integrations".into())) {
                    eprintln!("[oauth] open home: {e}");
                }
            }
        }
        Err(e) => {
            eprintln!("[oauth] {} claim failed: {e}", p.as_str());
            let code = match e {
                AppError::Code(c) => c,
                AppError::Http(_) => "network",
                _ => "invalid",
            };
            events::integration_changed(&app, Some(p.as_str()), false, Some(code));
        }
    }
}

/// `POST /oauth/claim` → token → `me()` → keychain. A rejected claim (used or expired
/// handoff) is `oauthExpired`.
async fn claim_token(app: &AppHandle, p: Provider, state: &str, verifier: &str) -> AppResult<()> {
    backend::ensure_network(app)?;
    let b = backend::config().ok_or(AppError::Code("notConfigured"))?;
    let res = backend::http()
        .post(format!("{}/functions/v1/oauth/claim", b.url))
        .header("apikey", b.key)
        .json(&json!({ "state": state, "verifier": verifier }))
        .send()
        .await?;
    if res.status().is_client_error() {
        return Err(AppError::Code("oauthExpired"));
    }
    let token: BackendToken = res.error_for_status()?.json().await?;
    let account_label = me(p, &token.access_token).await?;
    secrets::set_json(
        p.as_str(),
        &ProviderToken {
            method: "oauth".into(),
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            expires_at: token.expires_at,
            account_label,
            needs_reconnect: false,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handoff_states() {
        let t0 = Instant::now();
        let mut h = Handoffs::default();
        h.insert("s1".into(), Provider::Notion, "v1".into(), t0);
        h.insert("s2".into(), Provider::Asana, "v2".into(), t0);
        assert_eq!(h.take("s1", t0 + Duration::from_secs(5)), Claim::Go(Provider::Notion, "v1".into()));
        // A repeated delivery of a handled state is silent.
        assert_eq!(h.take("s1", t0 + Duration::from_secs(6)), Claim::Ignore);
        assert_eq!(h.take("never", t0), Claim::Expired(None));
        assert_eq!(h.take("s2", t0 + TTL + Duration::from_secs(1)), Claim::Expired(Some(Provider::Asana)));
        assert_eq!(h.take("s2", t0 + TTL + Duration::from_secs(2)), Claim::Ignore);
    }

    #[test]
    fn pkce_values() {
        let s = random_b64().unwrap();
        assert_eq!(s.len(), 43);
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        // RFC 7636 appendix B.
        assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        let url = start_url("https://x.supabase.co", Provider::Asana, "st", "ch").unwrap();
        assert_eq!(url.as_str(), "https://x.supabase.co/functions/v1/oauth/start?provider=asana&state=st&challenge=ch");
    }
}
