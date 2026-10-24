//! D13: Screen Recording and Accessibility need no permission on Windows; the microphone comes from
//! the CapabilityAccessManager consent registry (`mic_consent`); there is no calendar source (D10).
use windows::Win32::System::Registry::{RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::error::{AppError, AppResult};
use crate::model::PermissionsStatus;
use crate::platform::{mic_consent, wide};

const MIC: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";

fn reg_sz(root: HKEY, key: &str) -> Option<String> {
    let (k, v) = (wide(key), wide("Value"));
    let mut buf = [0u16; 32];
    let mut len = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(
            root,
            windows::core::PCWSTR(k.as_ptr()),
            windows::core::PCWSTR(v.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
    };
    r.is_ok().then(|| super::from_wide(&buf))
}

pub fn screen_recording_granted() -> bool {
    true
}
// The `request_*` no-ops keep the facade identical to macOS, whose `request` calls them.
#[allow(dead_code)]
pub fn request_screen_recording() {}
pub fn accessibility_trusted() -> bool {
    true
}
#[allow(dead_code)]
pub fn request_accessibility() {}

/// Co-pet's own consent entry: the package's in the MSIX package, else the "Let desktop apps
/// access your microphone" switch.
fn app_entry() -> String {
    match super::package::package_family_name() {
        Some(family) => format!(r"{MIC}\{family}"),
        None => format!(r"{MIC}\NonPackaged"),
    }
}

/// The device-wide switch (HKLM, set by an admin) overrides the user's own global switch when off.
pub fn microphone_status() -> &'static str {
    let device = reg_sz(HKEY_LOCAL_MACHINE, MIC);
    let user = reg_sz(HKEY_CURRENT_USER, MIC);
    let global = device.as_deref().filter(|v| v.eq_ignore_ascii_case("Deny")).or(user.as_deref());
    mic_consent::microphone_status(global, reg_sz(HKEY_CURRENT_USER, &app_entry()).as_deref())
}

/// Desktop apps get no in-app prompt: opens Settings › Privacy › Microphone when denied.
pub async fn request_microphone() -> bool {
    if microphone_status() == "granted" {
        return true;
    }
    let _ = open_privacy_pane("microphone");
    false
}

/// D10: no calendar source on Windows.
pub fn calendar_status() -> &'static str {
    "unavailable"
}
#[allow(dead_code)]
pub async fn request_calendar() -> bool {
    false
}

pub fn status() -> PermissionsStatus {
    PermissionsStatus {
        screen_recording: true,
        accessibility: true,
        microphone: microphone_status().into(),
        calendar: calendar_status().into(),
    }
}

pub fn open_privacy_pane(kind: &str) -> AppResult<()> {
    let uri = match kind {
        "microphone" => "ms-settings:privacy-microphone",
        _ => return Err(AppError::Code("invalid")),
    };
    let (op, file) = (wide("open"), wide(uri));
    let r = unsafe {
        ShellExecuteW(
            None,
            windows::core::PCWSTR(op.as_ptr()),
            windows::core::PCWSTR(file.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    if r.0 as isize <= 32 {
        Err(AppError::msg("couldn't open Settings"))
    } else {
        Ok(())
    }
}

/// Only the microphone has anything to open; the other kinds are always granted on Windows.
pub fn request(kind: &str) -> AppResult<()> {
    match kind {
        "microphone" => open_privacy_pane(kind),
        "screenRecording" | "accessibility" | "calendar" => Ok(()),
        _ => Err(AppError::Code("invalid")),
    }
}
