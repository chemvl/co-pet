//! The sync master key. The first Mac makes a random master key and a recovery key; the server
//! keeps only the master key sealed under a key derived from the recovery key
//! (`public.sync_keys`, REST base `{url}/rest/v1`). Other Macs unwrap it with the recovery key.
//! Each Mac keeps the master key in the keychain as `sync.key.<userId>` (base64).
use std::sync::Mutex;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::json;
use tauri::{AppHandle, Manager};

use super::auth::{self, Session};
use super::crypto::{crockford_decode, crockford_encode, hkdf32, open, random32, seal};
use super::{send, server};
use crate::backend::{self, Backend};
use crate::db::settings;
use crate::error::{AppError, AppResult};
use crate::events;
use crate::model::KeyState;
use crate::secrets;

const RECOVERY_INFO: &str = "copet recovery v1";

/// The master key and recovery key between `create_recovery` and `confirm_recovery`.
#[derive(Default)]
pub struct PendingKey(Mutex<Option<([u8; 32], [u8; 32])>>);

impl PendingKey {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<([u8; 32], [u8; 32])>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Keychain account of a user's master key.
pub(crate) fn key_account(user_id: &str) -> String {
    format!("sync.key.{user_id}")
}

/// `base64(seal(hkdf32(rk, user_id, "copet recovery v1"), user_id, mk))`.
fn wrap(rk: &[u8; 32], user_id: &str, mk: &[u8; 32]) -> String {
    let kek = hkdf32(rk, user_id.as_bytes(), RECOVERY_INFO);
    STANDARD.encode(seal(&kek, user_id.as_bytes(), mk))
}

/// Reverses [`wrap`]; any mismatch (wrong recovery key or user, damaged value) is `decrypt`.
fn unwrap(rk: &[u8; 32], user_id: &str, wrapped: &str) -> AppResult<[u8; 32]> {
    let sealed = STANDARD.decode(wrapped).map_err(|_| AppError::Code("decrypt"))?;
    let kek = hkdf32(rk, user_id.as_bytes(), RECOVERY_INFO);
    open(&kek, user_id.as_bytes(), &sealed)?.try_into().map_err(|_| AppError::Code("decrypt"))
}

#[derive(Deserialize)]
struct WrappedRow {
    wrapped_key: String,
}

/// PostgREST as the signed-in user (row-level security limits it to their own row).
fn rest(b: &Backend, s: &Session, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    req.header("apikey", b.key).bearer_auth(&s.access_token)
}

/// The user's wrapped master key; `None` before any Mac turned sync on.
pub(crate) async fn fetch_wrapped(b: &Backend, s: &Session) -> AppResult<Option<String>> {
    let req = rest(b, s, backend::http().get(format!("{}/rest/v1/sync_keys?select=wrapped_key", b.url)));
    let rows: Vec<WrappedRow> = send(req).await?.error_for_status()?.json().await?;
    Ok(rows.into_iter().next().map(|r| r.wrapped_key))
}

/// Uploads `mk` wrapped by `rk`. `keyExists` when another Mac turned sync on first.
pub(crate) async fn upload(b: &Backend, s: &Session, mk: &[u8; 32], rk: &[u8; 32]) -> AppResult<()> {
    let req = rest(b, s, backend::http().post(format!("{}/rest/v1/sync_keys", b.url)))
        .header("Prefer", "return=minimal")
        .json(&json!({ "wrapped_key": wrap(rk, &s.user_id, mk) }));
    let res = send(req).await?;
    if res.status() == StatusCode::CONFLICT {
        return Err(AppError::Code("keyExists"));
    }
    res.error_for_status()?;
    Ok(())
}

/// The master key from the typed recovery key. `badRecoveryKey` when the text isn't a
/// recovery key or doesn't unwrap this user's key; `notFound` when no Mac turned sync on yet.
pub(crate) async fn recover(b: &Backend, s: &Session, text: &str) -> AppResult<[u8; 32]> {
    let rk = crockford_decode(text).ok_or(AppError::Code("badRecoveryKey"))?;
    let wrapped = fetch_wrapped(b, s).await?.ok_or(AppError::Code("notFound"))?;
    unwrap(&rk, &s.user_id, &wrapped).map_err(|_| AppError::Code("badRecoveryKey"))
}

/// `needsSetup` or `needsRecovery` from the server's row.
pub(crate) async fn remote_state(b: &Backend, s: &Session) -> AppResult<KeyState> {
    Ok(match fetch_wrapped(b, s).await? {
        None => KeyState::NeedsSetup,
        Some(_) => KeyState::NeedsRecovery,
    })
}

// ---------- app ----------

/// `none` signed out, `ready` with the master key on this Mac, else what the server holds;
/// `unknown` when the server can't be asked (offline, Local-only mode, server error).
pub async fn key_state(app: &AppHandle) -> KeyState {
    match local_or_remote_state(app).await {
        Ok(state) => state,
        Err(AppError::Code("signedOut")) => KeyState::None,
        Err(e) => {
            eprintln!("[account] key state: {e}");
            KeyState::Unknown
        }
    }
}

async fn local_or_remote_state(app: &AppHandle) -> AppResult<KeyState> {
    if auth::stored_session()?.is_none() {
        return Ok(KeyState::None);
    }
    if master_key(app)?.is_some() {
        return Ok(KeyState::Ready);
    }
    let b = server(app)?;
    let s = auth::session(app).await?;
    remote_state(&b, &s).await
}

/// Makes a new master key and recovery key, kept in memory until [`confirm_recovery`].
/// Returns the recovery key text. `signedOut` without a session.
pub fn create_recovery(app: &AppHandle) -> AppResult<String> {
    if auth::stored_session()?.is_none() {
        return Err(AppError::Code("signedOut"));
    }
    let rk = random32();
    *app.state::<PendingKey>().lock() = Some((random32(), rk));
    Ok(crockford_encode(&rk))
}

/// Uploads the pending master key wrapped by the recovery key and keeps it on this Mac.
/// `invalid` without [`create_recovery`] first; `keyExists` when another Mac was first.
pub async fn confirm_recovery(app: &AppHandle) -> AppResult<()> {
    let (mk, rk) = app.state::<PendingKey>().lock().ok_or(AppError::Code("invalid"))?;
    let b = server(app)?;
    let s = auth::session(app).await?;
    upload(&b, &s, &mk, &rk).await?;
    store(app, &s.user_id, &mk)
}

/// Unlocks sync on this Mac with the recovery key saved on the first one.
pub async fn enter_recovery(app: &AppHandle, text: &str) -> AppResult<()> {
    let b = server(app)?;
    let s = auth::session(app).await?;
    let mk = recover(&b, &s, text).await?;
    store(app, &s.user_id, &mk)
}

fn store(app: &AppHandle, user_id: &str, mk: &[u8; 32]) -> AppResult<()> {
    secrets::set(&key_account(user_id), &STANDARD.encode(mk))?;
    forget_pending(app);
    events::account_changed(app, None);
    crate::sync::poke(app);
    Ok(())
}

pub(crate) fn forget_pending(app: &AppHandle) {
    *app.state::<PendingKey>().lock() = None;
}

/// The master key on this Mac; `None` signed out or before sync was turned on here.
pub fn master_key(app: &AppHandle) -> AppResult<Option<[u8; 32]>> {
    let Some(user_id) = settings::read::<Option<String>>(app, "sync.userId")? else { return Ok(None) };
    let Some(text) = secrets::get(&key_account(&user_id))? else { return Ok(None) };
    let key = STANDARD.decode(text.trim()).ok().and_then(|b| <[u8; 32]>::try_from(b).ok());
    key.map(Some).ok_or_else(|| AppError::msg(format!("keychain {}: not a key", key_account(&user_id))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_unwrap_round_trip() {
        let (mk, rk) = (random32(), random32());
        let wrapped = wrap(&rk, "u1", &mk);
        assert_eq!(unwrap(&rk, "u1", &wrapped).unwrap(), mk);
        // Recovery key typed back from its text.
        let typed = crockford_encode(&rk).to_lowercase().replace('-', " ");
        assert_eq!(unwrap(&crockford_decode(&typed).unwrap(), "u1", &wrapped).unwrap(), mk);
    }

    #[test]
    fn unwrap_rejects_other_keys_users_and_damage() {
        let (mk, rk) = (random32(), random32());
        let wrapped = wrap(&rk, "u1", &mk);
        let code = |r: AppResult<[u8; 32]>| match r {
            Err(AppError::Code(c)) => c,
            _ => panic!("expected an error code"),
        };
        assert_eq!(code(unwrap(&random32(), "u1", &wrapped)), "decrypt");
        assert_eq!(code(unwrap(&rk, "u2", &wrapped)), "decrypt");
        assert_eq!(code(unwrap(&rk, "u1", "not base64!")), "decrypt");
        assert_eq!(code(unwrap(&rk, "u1", &STANDARD.encode(seal(&random32(), b"u1", &mk)))), "decrypt");
        // A sealed value that isn't 32 bytes.
        let kek = hkdf32(&rk, b"u1", RECOVERY_INFO);
        assert_eq!(code(unwrap(&rk, "u1", &STANDARD.encode(seal(&kek, b"u1", b"short")))), "decrypt");
    }

    #[test]
    fn rows_and_accounts() {
        let rows: Vec<WrappedRow> = serde_json::from_str(r#"[{"wrapped_key":"AQAA"}]"#).unwrap();
        assert_eq!(rows[0].wrapped_key, "AQAA");
        assert!(serde_json::from_str::<Vec<WrappedRow>>("[]").unwrap().is_empty());
        assert_eq!(key_account("u1"), "sync.key.u1");
    }
}
