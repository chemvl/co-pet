//! OS secret store: macOS keychain, Windows DPAPI files. Holds the Notion and
//! Asana tokens (`notion`, `asana`: JSON `ProviderToken`), the AI provider keys (`ai.anthropic`,
//! `ai.openai`, `ai.gemini`: the raw key), and from M8 the Supabase session and sync keys.
//! Values never go to SQLite or the logs.
//!
//! Reads are cached for the life of the process: only this app writes these entries, and every
//! keychain read can raise a macOS password prompt when the entry's access list doesn't trust the
//! running binary (ad-hoc builds, or "Allow" instead of "Always Allow"). With the cache that is
//! one prompt per entry per launch instead of one per sync tick or screen change.
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::platform::secret_store;

/// account → value (`None`: no entry). Failed reads (a denied prompt) aren't cached. The lock is
/// held across keychain calls so two threads never prompt for the same entry at once.
static CACHE: LazyLock<Mutex<HashMap<String, Option<String>>>> = LazyLock::new(Default::default);

/// The keychain value of the `notion` and `asana` accounts. No `Debug`, so it can't be logged.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderToken {
    /// `"oauth" | "token"`.
    pub method: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Epoch ms; `None` when the token doesn't expire.
    pub expires_at: Option<i64>,
    pub account_label: String,
    pub needs_reconnect: bool,
}

fn cache() -> std::sync::MutexGuard<'static, HashMap<String, Option<String>>> {
    CACHE.lock().unwrap_or_else(|e| e.into_inner())
}

/// `None` when the account has no entry.
pub fn get(account: &str) -> AppResult<Option<String>> {
    let mut cache = cache();
    if let Some(v) = cache.get(account) {
        return Ok(v.clone());
    }
    let v = match secret_store::get(account) {
        Ok(Some(bytes)) => Some(
            String::from_utf8(bytes).map_err(|_| AppError::msg(format!("keychain {account}: not UTF-8")))?,
        ),
        Ok(None) => None,
        Err(e) => return Err(AppError::msg(format!("keychain read {account}: {e}"))),
    };
    cache.insert(account.to_owned(), v.clone());
    Ok(v)
}

/// Creates or overwrites the entry.
pub fn set(account: &str, value: &str) -> AppResult<()> {
    let mut cache = cache();
    secret_store::set(account, value.as_bytes())
        .map_err(|e| AppError::msg(format!("keychain write {account}: {e}")))?;
    cache.insert(account.to_owned(), Some(value.to_owned()));
    Ok(())
}

/// A missing entry is not an error.
pub fn delete(account: &str) -> AppResult<()> {
    let mut cache = cache();
    match secret_store::delete(account) {
        Err(e) => {
            // The entry may or may not be gone: read it again next time.
            cache.remove(account);
            Err(AppError::msg(format!("keychain delete {account}: {e}")))
        }
        _ => {
            cache.insert(account.to_owned(), None);
            Ok(())
        }
    }
}

pub fn get_json<T: DeserializeOwned>(account: &str) -> AppResult<Option<T>> {
    get(account)?.map(|s| serde_json::from_str(&s)).transpose().map_err(Into::into)
}

pub fn set_json<T: Serialize>(account: &str, v: &T) -> AppResult<()> {
    set(account, &serde_json::to_string(v)?)
}
