//! Installed apps: the picker location, an app's id (bundle id) and its display name.
use std::path::Path;

use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSBundle, NSString, NSURL};

use crate::platform::AppPicker;

/// Where and what the "exclude an app" dialog picks: `.app` bundles in /Applications.
pub fn picker() -> AppPicker {
    AppPicker { dir: "/Applications".into(), filter_name: "Applications", extensions: &["app"] }
}

/// The app's id (bundle id on macOS, lowercase exe name on Windows); `None` when `path` isn't an app.
pub fn app_id(app_path: &Path) -> Option<String> {
    let bundle = NSBundle::bundleWithPath(&NSString::from_str(app_path.to_str()?))?;
    Some(bundle.bundleIdentifier()?.to_string())
}

/// The installed app's display name for `app_id` (bundle id on macOS, lowercase exe name on
/// Windows); `None` when not installed.
pub fn display_name(app_id: &str) -> Option<String> {
    NSWorkspace::sharedWorkspace()
        .URLForApplicationWithBundleIdentifier(&NSString::from_str(app_id))
        .and_then(|url| app_name(&url))
}

/// `CFBundleDisplayName`, else `CFBundleName`, else the bundle's file name without `.app`.
fn app_name(url: &NSURL) -> Option<String> {
    if let Some(bundle) = NSBundle::bundleWithURL(url) {
        for key in ["CFBundleDisplayName", "CFBundleName"] {
            let value = bundle.objectForInfoDictionaryKey(&NSString::from_str(key));
            if let Some(name) = value.and_then(|v| v.downcast::<NSString>().ok()) {
                let name = name.to_string();
                if !name.trim().is_empty() {
                    return Some(name);
                }
            }
        }
    }
    let path = url.path()?.to_string();
    Path::new(&path).file_stem().map(|s| s.to_string_lossy().into_owned())
}
