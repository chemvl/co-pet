//! The app the user is working in. The pet panel never activates Co-pet, so during a drop or a
//! hotkey this is the source app.
use objc2_app_kit::NSWorkspace;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontApp {
    pub name: Option<String>,
    pub bundle_id: Option<String>,
    pub pid: i32,
}

pub fn frontmost_app() -> Option<FrontApp> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    Some(FrontApp {
        name: app.localizedName().map(|s| s.to_string()),
        bundle_id: app.bundleIdentifier().map(|s| s.to_string()),
        pid: app.processIdentifier(),
    })
}

/// Just the frontmost app's pid: no LaunchServices round trips for its name and bundle id.
pub fn frontmost_pid() -> Option<i32> {
    Some(NSWorkspace::sharedWorkspace().frontmostApplication()?.processIdentifier())
}
