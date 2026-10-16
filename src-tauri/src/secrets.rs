//! macOS keychain: generic passwords under the service `com.copet.app`. Holds the Notion and
//! Asana tokens (`notion`, `asana`: JSON `ProviderToken`), and from M8 the Supabase session and
//! sync keys. Values never go to SQLite or the logs.
use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

const SERVICE: &str = "com.copet.app";
/// `errSecItemNotFound`.
const ITEM_NOT_FOUND: i32 = -25300;

/// The keychain value of the `notion` and `asana` accounts. No `Debug`, so it can't be logged.
// Task 4.3: written by connect/OAuth and read by the exporters.
#[allow(dead_code)]
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

/// `None` when the account has no entry.
pub fn get(account: &str) -> AppResult<Option<String>> {
    match get_generic_password(SERVICE, account) {
        Ok(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| AppError::msg(format!("keychain {account}: not UTF-8"))),
        Err(e) if e.code() == ITEM_NOT_FOUND => Ok(None),
        Err(e) => Err(AppError::msg(format!("keychain read {account}: {e}"))),
    }
}

/// Creates or overwrites the entry.
pub fn set(account: &str, value: &str) -> AppResult<()> {
    set_generic_password(SERVICE, account, value.as_bytes())
        .map_err(|e| AppError::msg(format!("keychain write {account}: {e}")))
}

/// A missing entry is not an error.
pub fn delete(account: &str) -> AppResult<()> {
    match delete_generic_password(SERVICE, account) {
        Err(e) if e.code() != ITEM_NOT_FOUND => {
            Err(AppError::msg(format!("keychain delete {account}: {e}")))
        }
        _ => Ok(()),
    }
}

// Task 4.3: the provider tokens.
#[allow(dead_code)]
pub fn get_json<T: DeserializeOwned>(account: &str) -> AppResult<Option<T>> {
    get(account)?.map(|s| serde_json::from_str(&s)).transpose().map_err(Into::into)
}

// Task 4.3: the provider tokens.
#[allow(dead_code)]
pub fn set_json<T: Serialize>(account: &str, v: &T) -> AppResult<()> {
    set(account, &serde_json::to_string(v)?)
}
