//! macOS keychain: generic passwords under the service `com.copet.app`.
use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};

const SERVICE: &str = "com.copet.app";
/// `errSecItemNotFound`.
const ITEM_NOT_FOUND: i32 = -25300;

pub fn get(account: &str) -> Result<Option<Vec<u8>>, String> {
    match get_generic_password(SERVICE, account) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.code() == ITEM_NOT_FOUND => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn set(account: &str, value: &[u8]) -> Result<(), String> {
    set_generic_password(SERVICE, account, value).map_err(|e| e.to_string())
}

/// A missing entry is not an error.
pub fn delete(account: &str) -> Result<(), String> {
    match delete_generic_password(SERVICE, account) {
        Err(e) if e.code() != ITEM_NOT_FOUND => Err(e.to_string()),
        _ => Ok(()),
    }
}
