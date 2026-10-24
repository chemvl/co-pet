//! The server side of sync. `Remote` is what a run needs: push sealed rows, pull them by `seq`,
//! put and get sealed files. `SupabaseRemote` talks to PostgREST (`records`, `rpc/sync_push`)
//! and Storage (bucket `blobs`, objects `<uid>/<hmac name>`); `MemoryRemote` stands in for it
//! in tests.
use std::time::Duration;

use bytes::Bytes;
use reqwest::header::CONTENT_TYPE;
use reqwest::{RequestBuilder, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::account::auth::{self, Session};
use crate::backend::{self, Backend};
use crate::error::AppError;

/// Files may take longer than the shared client's 30 s request timeout.
const BLOB_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// One `records` row. `payload` is the sealed row; `seq` is the server's order (unused on push).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct RemoteRow {
    pub tbl: String,
    pub id: String,
    pub updated_at: i64,
    pub device_id: String,
    pub deleted: bool,
    pub payload: String,
    pub seq: i64,
}

/// `Server` is any other failure: a server answer (its body) or a local error.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SyncError {
    Offline,
    SignedOut,
    Server(String),
}

impl From<AppError> for SyncError {
    fn from(e: AppError) -> Self {
        match e {
            AppError::Code("network") => SyncError::Offline,
            AppError::Code("signedOut") => SyncError::SignedOut,
            e => SyncError::Server(e.to_string()),
        }
    }
}

impl From<rusqlite::Error> for SyncError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::from(e).into()
    }
}

/// Native `async fn`, used generically only (no `dyn`).
pub(crate) trait Remote {
    /// Stores rows unless the server's copy wins on `(updated_at, device_id)`; how many it took.
    async fn push(&self, rows: &[RemoteRow]) -> Result<u32, SyncError>;
    /// Up to `limit` rows with `seq > after`, in `seq` order.
    async fn pull(&self, after: i64, limit: usize) -> Result<Vec<RemoteRow>, SyncError>;
    /// Creates or replaces the object `name`.
    async fn put_blob(&self, name: &str, bytes: Vec<u8>) -> Result<(), SyncError>;
    /// `None` when there is no object `name`.
    async fn get_blob(&self, name: &str) -> Result<Option<Vec<u8>>, SyncError>;
}

/// Where `SupabaseRemote` gets the signed-in user's session.
pub(crate) trait Credentials {
    async fn session(&self) -> Result<Session, SyncError>;
    /// A new session after the server refused the access token `rejected`.
    async fn renew(&self, rejected: &str) -> Result<Session, SyncError>;
}

/// The session in the keychain, refreshed when due (`account::auth`).
impl Credentials for AppHandle {
    async fn session(&self) -> Result<Session, SyncError> {
        Ok(auth::session(self).await?)
    }

    async fn renew(&self, rejected: &str) -> Result<Session, SyncError> {
        Ok(auth::session_after_rejection(self, rejected).await?)
    }
}

pub(crate) struct SupabaseRemote<C = AppHandle> {
    b: Backend,
    creds: C,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PushRow<'a> {
    tbl: &'a str,
    id: &'a str,
    updated_at: i64,
    device_id: &'a str,
    deleted: bool,
    payload: &'a str,
}

#[derive(Serialize)]
struct PushBody<'a> {
    rows: Vec<PushRow<'a>>,
}

/// A transport failure (offline, DNS, timeout, connection dropped) is `Offline`.
fn transport(e: reqwest::Error) -> SyncError {
    eprintln!("[sync] {e}");
    SyncError::Offline
}

/// A response read to its end.
struct Answer {
    status: StatusCode,
    body: Bytes,
}

impl Answer {
    /// The body of a 2xx, else `Server` with the status and body.
    fn ok(self) -> Result<Bytes, SyncError> {
        if self.status.is_success() {
            return Ok(self.body);
        }
        Err(SyncError::Server(format!("{}: {}", self.status, String::from_utf8_lossy(&self.body))))
    }

    fn json<T: DeserializeOwned>(self) -> Result<T, SyncError> {
        serde_json::from_slice(&self.ok()?).map_err(|e| SyncError::Server(format!("unexpected answer: {e}")))
    }
}

#[derive(Deserialize)]
struct StorageError {
    error: String,
}

/// Whether the server refused the access token: PostgREST answers `401`, Storage `400` with
/// `{"statusCode":"403","error":"Unauthorized",…}` (also its answer to a row-level security
/// refusal, which this client's own `<uid>/` paths never cause).
fn token_refused(status: StatusCode, body: &[u8]) -> bool {
    status == StatusCode::UNAUTHORIZED
        || (status == StatusCode::BAD_REQUEST
            && serde_json::from_slice::<StorageError>(body).is_ok_and(|e| e.error == "Unauthorized"))
}

impl<C: Credentials> SupabaseRemote<C> {
    pub fn new(b: Backend, creds: C) -> Self {
        SupabaseRemote { b, creds }
    }

    /// Sends what `build` makes for the current session, with `apikey` and the bearer token,
    /// and reads the answer. A refused token renews the session and tries once more; a second
    /// refusal is `SignedOut`.
    async fn send(&self, build: impl Fn(&Backend, &Session) -> RequestBuilder) -> Result<Answer, SyncError> {
        let mut session = self.creds.session().await?;
        let mut renewed = false;
        loop {
            let resp = build(&self.b, &session)
                .header("apikey", &self.b.key)
                .bearer_auth(&session.access_token)
                .send()
                .await
                .map_err(transport)?;
            let status = resp.status();
            let body = resp.bytes().await.map_err(transport)?;
            if !token_refused(status, &body) {
                return Ok(Answer { status, body });
            }
            if renewed {
                return Err(SyncError::SignedOut);
            }
            session = self.creds.renew(&session.access_token).await?;
            renewed = true;
        }
    }
}

impl<C: Credentials> Remote for SupabaseRemote<C> {
    async fn push(&self, rows: &[RemoteRow]) -> Result<u32, SyncError> {
        let body = PushBody {
            rows: rows
                .iter()
                .map(|r| PushRow {
                    tbl: &r.tbl,
                    id: &r.id,
                    updated_at: r.updated_at,
                    device_id: &r.device_id,
                    deleted: r.deleted,
                    payload: &r.payload,
                })
                .collect(),
        };
        self.send(|b, _| backend::http().post(format!("{}/rest/v1/rpc/sync_push", b.url)).json(&body))
            .await?
            .json()
    }

    async fn pull(&self, after: i64, limit: usize) -> Result<Vec<RemoteRow>, SyncError> {
        self.send(|b, _| {
            backend::http().get(format!(
                "{}/rest/v1/records?select=tbl,id,updated_at,device_id,deleted,payload,seq&seq=gt.{after}&order=seq.asc&limit={limit}",
                b.url
            ))
        })
        .await?
        .json()
    }

    async fn put_blob(&self, name: &str, bytes: Vec<u8>) -> Result<(), SyncError> {
        let body = Bytes::from(bytes);
        self.send(|b, s| {
            backend::http()
                .post(format!("{}/storage/v1/object/blobs/{}/{name}", b.url, s.user_id))
                .header("x-upsert", "true")
                .header(CONTENT_TYPE, "application/octet-stream")
                .timeout(BLOB_TIMEOUT)
                .body(body.clone())
        })
        .await?
        .ok()?;
        Ok(())
    }

    async fn get_blob(&self, name: &str) -> Result<Option<Vec<u8>>, SyncError> {
        let answer = self
            .send(|b, s| {
                backend::http()
                    .get(format!("{}/storage/v1/object/authenticated/blobs/{}/{name}", b.url, s.user_id))
                    .timeout(BLOB_TIMEOUT)
            })
            .await?;
        // Storage answers a missing object with `400` (`"statusCode":"404"`) or `404`.
        if matches!(answer.status, StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND) {
            return Ok(None);
        }
        Ok(Some(answer.ok()?.into()))
    }
}

#[cfg(test)]
pub(crate) use memory::MemoryRemote;

#[cfg(test)]
mod memory {
    use std::collections::{BTreeMap, HashMap};
    use std::sync::Mutex;

    use super::*;

    /// A fixed session (end-to-end tests); a refused token can't be renewed.
    impl Credentials for Session {
        async fn session(&self) -> Result<Session, SyncError> {
            Ok(self.clone())
        }

        async fn renew(&self, _rejected: &str) -> Result<Session, SyncError> {
            Err(SyncError::SignedOut)
        }
    }

    /// The server in memory: `sync_push`'s rule (a row replaces the stored one only when its
    /// `(updated_at, device_id)` is greater, taking the next `seq`) and a blob map.
    #[derive(Default)]
    pub(crate) struct MemoryRemote(Mutex<Memory>);

    #[derive(Default)]
    struct Memory {
        rows: BTreeMap<(String, String), RemoteRow>,
        seq: i64,
        blobs: HashMap<String, Vec<u8>>,
    }

    impl MemoryRemote {
        fn lock(&self) -> std::sync::MutexGuard<'_, Memory> {
            self.0.lock().unwrap_or_else(|e| e.into_inner())
        }

        /// Every stored row, in `seq` order.
        pub fn rows(&self) -> Vec<RemoteRow> {
            let mut rows: Vec<RemoteRow> = self.lock().rows.values().cloned().collect();
            rows.sort_by_key(|r| r.seq);
            rows
        }

        pub fn row(&self, tbl: &str, id: &str) -> Option<RemoteRow> {
            self.lock().rows.get(&(tbl.to_string(), id.to_string())).cloned()
        }

        pub fn blobs(&self) -> HashMap<String, Vec<u8>> {
            self.lock().blobs.clone()
        }
    }

    impl Remote for MemoryRemote {
        async fn push(&self, rows: &[RemoteRow]) -> Result<u32, SyncError> {
            let mut m = self.lock();
            let mut n = 0;
            for r in rows {
                let key = (r.tbl.clone(), r.id.clone());
                let wins = m.rows.get(&key).is_none_or(|t| (r.updated_at, r.device_id.as_str()) > (t.updated_at, t.device_id.as_str()));
                if wins {
                    m.seq += 1;
                    let seq = m.seq;
                    m.rows.insert(key, RemoteRow { seq, ..r.clone() });
                    n += 1;
                }
            }
            Ok(n)
        }

        async fn pull(&self, after: i64, limit: usize) -> Result<Vec<RemoteRow>, SyncError> {
            Ok(self.rows().into_iter().filter(|r| r.seq > after).take(limit).collect())
        }

        async fn put_blob(&self, name: &str, bytes: Vec<u8>) -> Result<(), SyncError> {
            self.lock().blobs.insert(name.to_string(), bytes);
            Ok(())
        }

        async fn get_blob(&self, name: &str) -> Result<Option<Vec<u8>>, SyncError> {
            Ok(self.lock().blobs.get(name).cloned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refused_tokens_from_both_services() {
        // Bodies as the local Supabase stack answers them.
        let storage_jwt = br#"{"statusCode":"403","error":"Unauthorized","message":"JWS Protected Header is invalid"}"#;
        let storage_missing = br#"{"statusCode":"404","error":"not_found","message":"Object not found"}"#;
        let postgrest = br#"{"code":"PGRST301","message":"Expected 3 parts in JWT; got 1"}"#;
        assert!(token_refused(StatusCode::UNAUTHORIZED, postgrest));
        assert!(token_refused(StatusCode::BAD_REQUEST, storage_jwt));
        assert!(!token_refused(StatusCode::BAD_REQUEST, storage_missing));
        assert!(!token_refused(StatusCode::BAD_REQUEST, b"not json"));
        assert!(!token_refused(StatusCode::OK, storage_jwt));
    }

    #[test]
    fn app_errors_map_to_sync_errors() {
        assert_eq!(SyncError::from(AppError::Code("network")), SyncError::Offline);
        assert_eq!(SyncError::from(AppError::Code("signedOut")), SyncError::SignedOut);
        assert_eq!(SyncError::from(AppError::Code("localOnly")), SyncError::Server("localOnly".into()));
    }
}
