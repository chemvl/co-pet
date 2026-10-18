//! The app the user is working in. The pet window never activates Co-pet, so during a drop or a
//! hotkey this is the source app.
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontApp {
    pub name: Option<String>,
    /// Lowercase exe file name on Windows (bundle id on macOS).
    pub bundle_id: Option<String>,
    pub pid: i32,
}

pub fn frontmost_pid() -> Option<i32> {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid)) };
    (pid != 0).then_some(pid as i32)
}

pub fn frontmost_app() -> Option<FrontApp> {
    let pid = frontmost_pid()?;
    let exe = super::exe_path(pid as u32);
    Some(FrontApp {
        name: exe.as_deref().and_then(super::exe_description),
        bundle_id: exe.as_deref().and_then(super::app_id_of),
        pid,
    })
}
