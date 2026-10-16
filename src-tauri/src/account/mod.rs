//! The Co-pet account: Supabase sign-in (`auth`), the end-to-end encryption keys (`keys`,
//! `crypto`), the account status and Delete cloud data. The session and keys live only in the
//! keychain; settings hold `sync.userId`, `sync.email` and `sync.deviceName`.
pub mod auth;
pub mod crypto;
pub mod keys;

use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::backend::{self, Backend};
use crate::data;
use crate::db::{settings, Db};
use crate::error::{AppError, AppResult};
use crate::events;
use crate::model::AccountStatus;
use crate::sync;

/// Accepted like Delete everything's phrases (`xoá` is the older Vietnamese spelling).
pub(crate) const CLOUD_PHRASES: [&str; 3] = ["delete cloud data", "xoá dữ liệu đám mây", "xóa dữ liệu đám mây"];

/// The backend for an account call: `localOnly` while Local-only mode is on, `notConfigured`
/// in a build without a Supabase project.
fn server(app: &AppHandle) -> AppResult<Backend> {
    backend::ensure_network(app)?;
    backend::config().ok_or(AppError::Code("notConfigured"))
}

/// Sends `req`; a transport failure (offline, DNS, timeout) is `network`.
async fn send(req: reqwest::RequestBuilder) -> AppResult<reqwest::Response> {
    req.send().await.map_err(|e| {
        eprintln!("[account] {e}");
        AppError::Code("network")
    })
}

pub async fn status(app: &AppHandle) -> AppResult<AccountStatus> {
    let (user_id, email, device_name, last_synced_at) = {
        let db = app.state::<Db>();
        let conn = db.conn();
        (
            settings::get::<Option<String>>(&conn, "sync.userId")?,
            settings::get::<Option<String>>(&conn, "sync.email")?,
            settings::get::<Option<String>>(&conn, "sync.deviceName")?,
            sync::last_synced_at(&conn)?,
        )
    };
    Ok(AccountStatus {
        configured: backend::config().is_some(),
        signed_in: auth::stored_session()?.is_some(),
        email,
        user_id,
        key_state: keys::key_state(app).await,
        device_name: device_name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| sysinfo::System::host_name().unwrap_or_default()),
        last_synced_at,
    })
}

/// Deletes the account and everything synced (the `delete-account` function), then signs out,
/// forgets `sync.userId` and this Mac's sync progress (`sync_state`, `sync_blobs`,
/// `sync_outbox`), so this Mac may sign in to any account. This Mac's data stays.
/// Error `confirmation` when the typed phrase doesn't match.
pub async fn delete_cloud_data(app: &AppHandle, confirmation: &str) -> AppResult<()> {
    if !data::phrase_ok(confirmation, &CLOUD_PHRASES) {
        return Err(AppError::Code("confirmation"));
    }
    let b = server(app)?;
    let token = auth::access_token(app).await?;
    delete_account(&b, &token).await?;
    auth::forget_session(app).await?;
    settings::set(app, "sync.userId", &Value::Null)?;
    sync::forget(app).await?;
    events::account_changed(app, None);
    sync::poke(app);
    Ok(())
}

async fn delete_account(b: &Backend, access_token: &str) -> AppResult<()> {
    let req = backend::http()
        .post(format!("{}/functions/v1/delete-account", b.url))
        .header("apikey", b.key)
        .bearer_auth(access_token);
    send(req).await?.error_for_status()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    use super::crypto::{crockford_encode, random32};
    use super::*;
    use crate::model::KeyState;

    fn code<T>(r: AppResult<T>) -> &'static str {
        match r {
            Err(AppError::Code(c)) => c,
            Err(e) => panic!("expected an error code, got {e}"),
            Ok(_) => panic!("expected an error code, got Ok"),
        }
    }

    /// The newest 6-digit code Mailpit holds for `email` that isn't in `seen`.
    async fn mailed_code(mail: &str, email: &str, seen: &[String]) -> String {
        let digits = regex::Regex::new(r"\b(\d{6})\b").unwrap();
        let http = backend::http();
        for _ in 0..40 {
            let found: serde_json::Value = http
                .get(format!("{mail}/api/v1/search"))
                .query(&[("query", format!("to:\"{email}\""))])
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            for m in found["messages"].as_array().into_iter().flatten() {
                let id = m["ID"].as_str().unwrap();
                let msg: serde_json::Value =
                    http.get(format!("{mail}/api/v1/message/{id}")).send().await.unwrap().json().await.unwrap();
                let text = msg["Text"].as_str().unwrap_or_default();
                if let Some(c) = digits.captures(text).map(|c| c[1].to_string()) {
                    if !seen.contains(&c) {
                        return c;
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        panic!("no code mailed to {email}");
    }

    /// The account flow against a local Supabase stack, without the app or the keychain (an
    /// in-memory map stands in for it). Not run by default:
    /// `COPET_E2E_URL=http://127.0.0.1:<api port> COPET_E2E_KEY=<publishable/anon key>
    ///  COPET_E2E_MAIL=http://127.0.0.1:<mailpit port>
    ///  cargo test --lib account::tests -- --ignored --nocapture`
    /// (needs the `sync_keys` migration and `supabase functions serve delete-account`).
    #[tokio::test]
    #[ignore = "needs a local Supabase stack (COPET_E2E_URL, COPET_E2E_KEY, COPET_E2E_MAIL)"]
    async fn local_stack_end_to_end() {
        let env = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set"));
        let b = Backend { url: env("COPET_E2E_URL").leak(), key: env("COPET_E2E_KEY").leak() };
        let mail = env("COPET_E2E_MAIL");
        let email = format!("e2e-{}@example.com", hex::encode(&random32()[..6]));
        let mut keychain: HashMap<String, String> = HashMap::new();

        // Mac A: email code sign-in.
        auth::request_code(&b, &email).await.unwrap();
        assert_eq!(code(auth::request_code(&b, &email).await), "rateLimited");
        println!("send_email_code ok; immediate resend -> rateLimited");
        let first = mailed_code(&mail, &email, &[]).await;
        let wrong = if first == "000000" { "111111" } else { "000000" };
        assert_eq!(code(auth::verify(&b, &email, wrong).await), "badCode");
        let a = auth::verify(&b, &email, &first).await.unwrap();
        assert_eq!(a.email.as_deref(), Some(email.as_str()));
        assert!(a.expires_at > crate::db::now_ms());
        keychain.insert(auth::SESSION.into(), serde_json::to_string(&a).unwrap());
        println!("verify: wrong code -> badCode; right code -> session for user {}", a.user_id);

        let refreshed = auth::refresh(&b, &a.refresh_token).await.unwrap();
        assert_eq!(refreshed.user_id, a.user_id);
        assert_ne!(refreshed.refresh_token, a.refresh_token);
        assert_eq!(code(auth::refresh(&b, "not-a-refresh-token").await), "signedOut");
        assert_eq!(code(auth::exchange_code(&b, "not-an-auth-code", "verifier").await), "oauthExpired");
        println!("refresh ok (same user, new refresh token); bad refresh -> signedOut; bad PKCE code -> oauthExpired");

        // Mac A turns sync on.
        assert_eq!(keys::remote_state(&b, &refreshed).await.unwrap(), KeyState::NeedsSetup);
        let (mk, rk) = (random32(), random32());
        let recovery_text = crockford_encode(&rk);
        keys::upload(&b, &refreshed, &mk, &rk).await.unwrap();
        keychain.insert(keys::key_account(&a.user_id), STANDARD.encode(mk));
        assert_eq!(code(keys::upload(&b, &refreshed, &random32(), &random32()).await), "keyExists");
        assert_eq!(keys::remote_state(&b, &refreshed).await.unwrap(), KeyState::NeedsRecovery);
        println!("sync_keys: [] -> needsSetup; POST -> ok; second POST -> keyExists; row -> needsRecovery");

        // Mac B: its own sign-in, then the recovery key typed back loosely.
        tokio::time::sleep(Duration::from_millis(1500)).await;
        auth::request_code(&b, &email).await.unwrap();
        let second = mailed_code(&mail, &email, &[first]).await;
        let mac_b = auth::verify(&b, &email, &second).await.unwrap();
        assert_eq!(mac_b.user_id, a.user_id);
        assert_ne!(mac_b.access_token, refreshed.access_token);
        let typed = recovery_text.to_lowercase().replace('-', " ");
        let unwrapped = keys::recover(&b, &mac_b, &typed).await.unwrap();
        assert_eq!(STANDARD.encode(unwrapped), keychain[&keys::key_account(&a.user_id)]);
        assert_eq!(code(keys::recover(&b, &mac_b, &crockford_encode(&random32())).await), "badRecoveryKey");
        assert_eq!(code(keys::recover(&b, &mac_b, "not a key").await), "badRecoveryKey");
        println!("Mac B: recovery key unwraps the same master key; other key / junk -> badRecoveryKey");

        // Delete cloud data from Mac B.
        delete_account(&b, &mac_b.access_token).await.unwrap();
        keychain.clear();
        assert_eq!(code(auth::refresh(&b, &mac_b.refresh_token).await), "signedOut");
        assert_eq!(keys::fetch_wrapped(&b, &mac_b).await.unwrap(), None);
        println!("delete-account ok; the user's refresh -> signedOut; sync_keys row gone");
    }
}
