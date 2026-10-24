//! Supabase Auth (`{url}/auth/v1`): email one-time codes, Google sign-in with PKCE through
//! `copet://auth/callback`, the session in the keychain and its refresh.
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

use super::{crypto, keys, send, server};
use crate::backend::{self, Backend};
use crate::db::{now_ms, settings};
use crate::error::{AppError, AppResult};
use crate::events;
use crate::export::oauth::challenge;
use crate::secrets;
use crate::sync;
use crate::windows;

/// Keychain account of the [`Session`].
pub(crate) const SESSION: &str = "supabase.session";
/// How long a Google sign-in started here may take.
const GOOGLE_TTL: Duration = Duration::from_secs(10 * 60);
/// Refresh this long before the access token expires.
const REFRESH_MARGIN_MS: i64 = 60_000;

/// The keychain value of `supabase.session`. No `Debug`, so it can't be logged.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    /// Epoch ms.
    pub expires_at: i64,
    pub user_id: String,
    pub email: Option<String>,
}

/// The PKCE verifier of the Google sign-in this process started, and when.
#[derive(Default)]
pub struct GooglePending(Mutex<Option<(String, Instant)>>);

impl GooglePending {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<(String, Instant)>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// One refresh at a time: a second caller waits and then sees the refreshed session instead of
/// spending the same refresh token again.
static REFRESH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// GoTrue's session response (`/verify`, `/token`).
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    /// Seconds.
    expires_in: i64,
    user: TokenUser,
}

#[derive(Deserialize)]
struct TokenUser {
    id: String,
    email: Option<String>,
}

impl TokenResponse {
    fn into_session(self, now: i64) -> Session {
        Session {
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            expires_at: now + self.expires_in.saturating_mul(1000),
            user_id: self.user.id,
            email: self.user.email.filter(|e| !e.is_empty()),
        }
    }
}

// ---------- requests (no app state) ----------

fn otp_body(email: &str) -> Value {
    json!({ "email": email, "create_user": true })
}

fn verify_body(email: &str, code: &str) -> Value {
    json!({ "type": "email", "email": email, "token": code })
}

fn pkce_body(code: &str, verifier: &str) -> Value {
    json!({ "auth_code": code, "code_verifier": verifier })
}

fn refresh_body(refresh_token: &str) -> Value {
    json!({ "refresh_token": refresh_token })
}

/// `/otp`: `429` is `rateLimited`, any other 4xx `invalid`.
fn otp_error(status: StatusCode) -> Option<&'static str> {
    match status.as_u16() {
        429 => Some("rateLimited"),
        400..=499 => Some("invalid"),
        _ => None,
    }
}

/// `{url}/auth/v1/authorize` for Google, coming back to `copet://auth/callback`.
fn authorize_url(base: &str, challenge: &str) -> String {
    format!(
        "{base}/auth/v1/authorize?provider=google&redirect_to=copet%3A%2F%2Fauth%2Fcallback\
         &code_challenge={challenge}&code_challenge_method=s256"
    )
}

/// Emails a 6-digit code, creating the user on first sign-in.
pub(crate) async fn request_code(b: &Backend, email: &str) -> AppResult<()> {
    let req = backend::http()
        .post(format!("{}/auth/v1/otp", b.url))
        .header("apikey", &b.key)
        .json(&otp_body(email));
    let res = send(req).await?;
    if let Some(code) = otp_error(res.status()) {
        return Err(AppError::Code(code));
    }
    res.error_for_status()?;
    Ok(())
}

/// `POST {url}/auth/v1{path}` returning a session; a 4xx is `client_error`.
async fn grant(b: &Backend, path: &str, body: &Value, client_error: &'static str) -> AppResult<Session> {
    let req = backend::http().post(format!("{}/auth/v1{path}", b.url)).header("apikey", &b.key).json(body);
    let res = send(req).await?;
    if res.status().is_client_error() {
        return Err(AppError::Code(client_error));
    }
    let token: TokenResponse = res.error_for_status()?.json().await?;
    Ok(token.into_session(now_ms()))
}

/// A wrong or expired code is `badCode`.
pub(crate) async fn verify(b: &Backend, email: &str, code: &str) -> AppResult<Session> {
    grant(b, "/verify", &verify_body(email, code), "badCode").await
}

/// A used or unknown auth code is `oauthExpired`.
pub(super) async fn exchange_code(b: &Backend, code: &str, verifier: &str) -> AppResult<Session> {
    grant(b, "/token?grant_type=pkce", &pkce_body(code, verifier), "oauthExpired").await
}

/// A revoked or used refresh token is `signedOut`.
pub(crate) async fn refresh(b: &Backend, refresh_token: &str) -> AppResult<Session> {
    grant(b, "/token?grant_type=refresh_token", &refresh_body(refresh_token), "signedOut").await
}

fn needs_refresh(expires_at: i64, now: i64) -> bool {
    expires_at - REFRESH_MARGIN_MS < now
}

/// A session may be stored unless this Mac already belongs to a different user.
fn may_adopt(current_user: Option<&str>, user_id: &str) -> bool {
    current_user.is_none_or(|u| u == user_id)
}

/// The verifier when the sign-in is still fresh.
fn claim_verifier(pending: Option<(String, Instant)>, now: Instant) -> Option<String> {
    pending.and_then(|(verifier, at)| (now.duration_since(at) < GOOGLE_TTL).then_some(verifier))
}

/// Whether the keychain still holds the session a refresh started from. Not when the user
/// signed out (or Delete everything ran, or a new sign-in replaced it) while the refresh was
/// in flight: its result must then not be stored, nor its refusal delete anything.
fn still_stored(stored: Option<&Session>, used: &Session) -> bool {
    stored.is_some_and(|s| s.refresh_token == used.refresh_token)
}

// ---------- app ----------

/// Rejects with `localOnly`, `notConfigured`, `network`, `rateLimited`, `invalid`.
pub async fn send_email_code(app: &AppHandle, email: &str) -> AppResult<()> {
    let b = server(app)?;
    request_code(&b, email.trim()).await
}

/// Signs in with the emailed code. Rejects with `badCode`, `otherAccount` and the network codes.
pub async fn verify_email_code(app: &AppHandle, email: &str, code: &str) -> AppResult<()> {
    let b = server(app)?;
    let session = verify(&b, email.trim(), code.trim()).await?;
    adopt(app, session)
}

/// Opens Google sign-in in the browser; it returns through `copet://auth/callback`.
pub fn google_start(app: &AppHandle) -> AppResult<()> {
    let b = server(app)?;
    let verifier = URL_SAFE_NO_PAD.encode(crypto::random32());
    let url = authorize_url(&b.url, &challenge(&verifier));
    *app.state::<GooglePending>().lock() = Some((verifier, Instant::now()));
    app.opener().open_url(url, None::<&str>).map_err(|e| AppError::msg(e.to_string()))
}

/// The `copet://auth/callback` deep link (`code` is missing when Google sign-in failed). A
/// failure emits `account-changed` with the error: `oauthExpired` (no fresh sign-in started
/// here, no code, or the code was refused), `otherAccount`, or a network code.
pub async fn google_finish(app: AppHandle, code: Option<String>) {
    let verifier = claim_verifier(app.state::<GooglePending>().lock().take(), Instant::now());
    let result = match (verifier, code) {
        (Some(verifier), Some(code)) => finish_google(&app, &code, &verifier).await,
        _ => Err(AppError::Code("oauthExpired")),
    };
    if let Err(e) = result {
        eprintln!("[account] Google sign-in failed: {e}");
        let code = match e {
            AppError::Code(c) => c,
            _ => "oauthExpired",
        };
        events::account_changed(&app, Some(code));
    }
}

async fn finish_google(app: &AppHandle, code: &str, verifier: &str) -> AppResult<()> {
    let b = server(app)?;
    let session = exchange_code(&b, code, verifier).await?;
    adopt(app, session)
}

/// Stores a new session unless this Mac's data belongs to another user (`otherAccount`), then
/// records the user, emits `account-changed` and shows Settings › Account.
fn adopt(app: &AppHandle, session: Session) -> AppResult<()> {
    let current: Option<String> = settings::read(app, "sync.userId")?;
    if !may_adopt(current.as_deref(), &session.user_id) {
        return Err(AppError::Code("otherAccount"));
    }
    secrets::set_json(SESSION, &session)?;
    settings::set(app, "sync.userId", &json!(session.user_id))?;
    settings::set(app, "sync.email", &json!(session.email))?;
    events::account_changed(app, None);
    sync::poke(app);
    if let Err(e) = windows::open_home(app, Some("settings/account".into())) {
        eprintln!("[account] open home: {e}");
    }
    Ok(())
}

pub(crate) fn stored_session() -> AppResult<Option<Session>> {
    secrets::get_json(SESSION)
}

/// The session with an access token valid for at least another minute, refreshed when needed.
/// `signedOut` without a session or when the refresh is refused (the session is then deleted
/// and `account-changed` emitted).
pub(crate) async fn session(app: &AppHandle) -> AppResult<Session> {
    fresh_session(app, None).await
}

/// Like [`session`], after the server refused the access token `rejected`: refreshes even
/// before expiry, unless another caller already replaced that token.
pub(crate) async fn session_after_rejection(app: &AppHandle, rejected: &str) -> AppResult<Session> {
    fresh_session(app, Some(rejected)).await
}

async fn fresh_session(app: &AppHandle, rejected: Option<&str>) -> AppResult<Session> {
    let b = server(app)?;
    let _turn = REFRESH.lock().await;
    let current = stored_session()?.ok_or(AppError::Code("signedOut"))?;
    let refused = rejected == Some(current.access_token.as_str());
    if !refused && !needs_refresh(current.expires_at, now_ms()) {
        return Ok(current);
    }
    let outcome = refresh(&b, &current.refresh_token).await;
    let stored = stored_session()?;
    if !still_stored(stored.as_ref(), &current) {
        return stored.ok_or(AppError::Code("signedOut"));
    }
    match outcome {
        Ok(fresh) => {
            secrets::set_json(SESSION, &fresh)?;
            Ok(fresh)
        }
        Err(AppError::Code("signedOut")) => {
            secrets::delete(SESSION)?;
            events::account_changed(app, None);
            Err(AppError::Code("signedOut"))
        }
        Err(e) => Err(e),
    }
}

pub async fn access_token(app: &AppHandle) -> AppResult<String> {
    Ok(session(app).await?.access_token)
}

/// Deletes the session and this user's sync key and clears `sync.email`. `sync.userId` stays,
/// so this Mac's data can't be merged into a different account.
pub async fn sign_out(app: &AppHandle) -> AppResult<()> {
    forget_session(app).await?;
    events::account_changed(app, None);
    sync::poke(app);
    Ok(())
}

/// [`sign_out`] without the event. Waits for a refresh in flight, so it can't store the
/// session again after this deleted it.
pub(crate) async fn forget_session(app: &AppHandle) -> AppResult<()> {
    let _turn = REFRESH.lock().await;
    secrets::delete(SESSION)?;
    if let Some(user_id) = settings::read::<Option<String>>(app, "sync.userId")? {
        secrets::delete(&keys::key_account(&user_id))?;
    }
    keys::forget_pending(app);
    settings::set(app, "sync.email", &Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_bodies() {
        assert_eq!(otp_body("a@b.co"), json!({ "email": "a@b.co", "create_user": true }));
        assert_eq!(verify_body("a@b.co", "123456"), json!({ "type": "email", "email": "a@b.co", "token": "123456" }));
        assert_eq!(pkce_body("c", "v"), json!({ "auth_code": "c", "code_verifier": "v" }));
        assert_eq!(refresh_body("r"), json!({ "refresh_token": "r" }));
    }

    #[test]
    fn otp_errors() {
        assert_eq!(otp_error(StatusCode::TOO_MANY_REQUESTS), Some("rateLimited"));
        assert_eq!(otp_error(StatusCode::UNPROCESSABLE_ENTITY), Some("invalid"));
        assert_eq!(otp_error(StatusCode::BAD_REQUEST), Some("invalid"));
        assert_eq!(otp_error(StatusCode::OK), None);
        assert_eq!(otp_error(StatusCode::INTERNAL_SERVER_ERROR), None);
    }

    #[test]
    fn token_response_becomes_session() {
        let raw = r#"{"access_token":"at","token_type":"bearer","expires_in":3600,"expires_at":1,
            "refresh_token":"rt","user":{"id":"u1","email":"a@b.co","aud":"authenticated"}}"#;
        let s = serde_json::from_str::<TokenResponse>(raw).unwrap().into_session(1_000);
        assert_eq!(
            (s.access_token.as_str(), s.refresh_token.as_str(), s.expires_at, s.user_id.as_str(), s.email.as_deref()),
            ("at", "rt", 3_601_000, "u1", Some("a@b.co"))
        );
        let raw = r#"{"access_token":"at","expires_in":60,"refresh_token":"rt","user":{"id":"u2","email":""}}"#;
        assert_eq!(serde_json::from_str::<TokenResponse>(raw).unwrap().into_session(0).email, None);
        // The keychain JSON is camelCase.
        let stored = serde_json::to_value(&s).unwrap();
        assert_eq!(stored["expiresAt"], 3_601_000);
        assert_eq!(stored["userId"], "u1");
    }

    #[test]
    fn refresh_a_minute_early() {
        assert!(!needs_refresh(100_000, 39_999));
        assert!(!needs_refresh(100_000, 40_000));
        assert!(needs_refresh(100_000, 40_001));
        assert!(needs_refresh(100_000, 200_000));
    }

    #[test]
    fn adopt_only_the_same_user() {
        assert!(may_adopt(None, "u1"));
        assert!(may_adopt(Some("u1"), "u1"));
        assert!(!may_adopt(Some("u1"), "u2"));
    }

    #[test]
    fn refreshed_session_kept_only_while_still_stored() {
        let s = |refresh: &str| Session {
            access_token: "at".into(),
            refresh_token: refresh.into(),
            expires_at: 0,
            user_id: "u1".into(),
            email: None,
        };
        let used = s("r1");
        assert!(still_stored(Some(&s("r1")), &used));
        // Signed out (or Delete everything) during the refresh.
        assert!(!still_stored(None, &used));
        // A new sign-in stored another session during the refresh.
        assert!(!still_stored(Some(&s("r2")), &used));
    }

    #[test]
    fn google_urls_and_verifier() {
        assert_eq!(
            authorize_url("https://x.supabase.co", "ch"),
            "https://x.supabase.co/auth/v1/authorize?provider=google&redirect_to=copet%3A%2F%2Fauth%2Fcallback\
             &code_challenge=ch&code_challenge_method=s256"
        );
        let t0 = Instant::now();
        assert_eq!(claim_verifier(Some(("v".into(), t0)), t0 + Duration::from_secs(599)), Some("v".into()));
        assert_eq!(claim_verifier(Some(("v".into(), t0)), t0 + GOOGLE_TTL), None);
        assert_eq!(claim_verifier(None, t0), None);
    }
}
