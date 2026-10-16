//! `copet://` deep links. macOS only delivers them to a bundled app (`tauri build`), not to
//! `tauri dev`.
use tauri::AppHandle;
use url::Url;

use crate::db::settings;
use crate::export::oauth;
use crate::windows;

#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    /// `copet://oauth/done?state=…[&error=…]`: the OAuth function handing an export
    /// connection back.
    OAuthDone { state: String, error: Option<String> },
    /// `copet://auth/callback?code=…`: Google sign-in handing back (Task 8.2). `code` is
    /// missing when the sign-in failed.
    AuthCallback { code: Option<String> },
    /// `copet://item/<id>`
    Item(String),
    /// `copet://task/<id>`
    Task(String),
    Unknown,
}

pub fn route(url: &Url) -> Route {
    if url.scheme() != "copet" {
        return Route::Unknown;
    }
    let query = |name: &str| url.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.into_owned());
    let path = url.path();
    match url.host_str() {
        Some("oauth") if path == "/done" => match query("state") {
            Some(state) => Route::OAuthDone { state, error: query("error") },
            None => Route::Unknown,
        },
        Some("auth") if path == "/callback" => Route::AuthCallback { code: query("code") },
        Some("item") => id(path).map_or(Route::Unknown, Route::Item),
        Some("task") => id(path).map_or(Route::Unknown, Route::Task),
        _ => Route::Unknown,
    }
}

/// The single path segment of `/<id>`; ids are UUIDs, so anything else can't name a row and
/// would only reach the Home router as a different route.
fn id(path: &str) -> Option<String> {
    let id = path.strip_prefix('/')?;
    (!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).then(|| id.to_string())
}

/// `scheme://host/path` only: the query carries OAuth `state` / `code`, which never go to logs.
fn redacted(url: &Url) -> String {
    format!("{}://{}{}", url.scheme(), url.host_str().unwrap_or(""), url.path())
}

/// Opens what the link names; `oauth/done` finishes an export connection. Item and task links
/// show onboarding instead while it isn't completed. The `auth` shape is handled by Task 8.2.
pub fn handle(app: &AppHandle, url: Url) {
    let onboarded = || settings::read::<bool>(app, "onboarding.completed").unwrap_or(false);
    let opened = match route(&url) {
        Route::OAuthDone { state, error } => {
            tauri::async_runtime::spawn(oauth::finish(app.clone(), state, error));
            return;
        }
        Route::Item(_) | Route::Task(_) if !onboarded() => windows::open_onboarding(app),
        Route::Item(id) => windows::open_home(app, Some(format!("item/{id}"))),
        Route::Task(id) => windows::open_home(app, Some(format!("tasks/open/{id}"))),
        _ => {
            eprintln!("[links] ignored {}", redacted(&url));
            return;
        }
    };
    if let Err(e) = opened {
        eprintln!("[links] {}: {e}", redacted(&url));
    }
}

#[cfg(test)]
mod tests {
    use super::{route, Route};

    fn r(s: &str) -> Route {
        route(&s.parse().unwrap())
    }

    #[test]
    fn routes_parse() {
        assert_eq!(
            r("copet://oauth/done?provider=notion&state=abc"),
            Route::OAuthDone { state: "abc".into(), error: None }
        );
        assert_eq!(
            r("copet://oauth/done?state=abc&error=expired"),
            Route::OAuthDone { state: "abc".into(), error: Some("expired".into()) }
        );
        assert_eq!(r("copet://oauth/done?error=expired"), Route::Unknown);
        assert_eq!(r("copet://auth/callback?code=c0de"), Route::AuthCallback { code: Some("c0de".into()) });
        assert_eq!(r("copet://auth/callback?error=access_denied"), Route::AuthCallback { code: None });
        let id = "0199f0a2-7c1e-7d3a-9b2f-1a2b3c4d5e6f";
        assert_eq!(r(&format!("copet://item/{id}")), Route::Item(id.into()));
        assert_eq!(r(&format!("copet://task/{id}")), Route::Task(id.into()));
        assert_eq!(r("copet://item/"), Route::Unknown);
        assert_eq!(r("copet://item/a/b"), Route::Unknown);
        assert_eq!(r("copet://task/%2Fsettings"), Route::Unknown);
        assert_eq!(r("copet://settings/general"), Route::Unknown);
        assert_eq!(r("copet://oauth/start"), Route::Unknown);
        assert_eq!(r("https://item/abc"), Route::Unknown);
    }
}
