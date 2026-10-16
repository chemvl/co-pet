//! The Supabase backend: its build-time address and key, one shared HTTP client for every
//! Phase 2 network call, and the `privacy.localOnly` gate in front of them.
use std::sync::LazyLock;
use std::time::Duration;

use tauri::AppHandle;

use crate::db::settings;
use crate::error::{AppError, AppResult};

pub struct Backend {
    pub url: &'static str,
    pub key: &'static str,
}

/// Set at build time by `COPET_SUPABASE_URL` and `COPET_SUPABASE_KEY`; `None` unless both are
/// non-empty. `url` has no trailing `/`.
pub fn config() -> Option<Backend> {
    parse(option_env!("COPET_SUPABASE_URL"), option_env!("COPET_SUPABASE_KEY"))
}

fn parse(url: Option<&'static str>, key: Option<&'static str>) -> Option<Backend> {
    let url = url?.trim().trim_end_matches('/');
    let key = key?.trim();
    (!url.is_empty() && !key.is_empty()).then_some(Backend { url, key })
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
    use super::parse;

    #[test]
    fn config_needs_both_values() {
        let b = parse(Some("https://x.supabase.co/"), Some("sb_publishable_1")).unwrap();
        assert_eq!((b.url, b.key), ("https://x.supabase.co", "sb_publishable_1"));
        assert!(parse(Some("https://x.supabase.co"), None).is_none());
        assert!(parse(None, Some("k")).is_none());
        assert!(parse(Some(""), Some("k")).is_none());
        assert!(parse(Some("https://x.supabase.co"), Some(" ")).is_none());
    }
}
