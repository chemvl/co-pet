//! MSIX package identity. Co-pet reaches Windows users as a Microsoft Store MSIX package;
//! `tauri dev` and local builds run unpackaged. A packaged run differs where the package model
//! does: open at login is the manifest's startup task (the HKCU `Run` key is virtualized), agents
//! run `copet-mcp` through its app execution alias, the data folder sits in the home folder
//! (`%APPDATA%` writes are virtualized) and microphone consent is per package.
use std::sync::LazyLock;

use windows::core::{HSTRING, PWSTR};
use windows::ApplicationModel::{StartupTask, StartupTaskState};
use windows::Win32::Foundation::{APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
use windows::Win32::Storage::Packaging::Appx::{GetCurrentPackageFamilyName, GetCurrentPackageFullName};

/// The manifest's `windows.startupTask` extension (`TaskId`).
const STARTUP_TASK: &str = "CoPetStartup";

static PACKAGED: LazyLock<bool> = LazyLock::new(|| {
    let mut len = 0u32;
    let rc = unsafe { GetCurrentPackageFullName(&mut len, None) };
    rc != APPMODEL_ERROR_NO_PACKAGE
});

static FAMILY: LazyLock<Option<String>> = LazyLock::new(|| {
    let mut len = 0u32;
    if unsafe { GetCurrentPackageFamilyName(&mut len, None) } != ERROR_INSUFFICIENT_BUFFER {
        return None;
    }
    let mut buf = vec![0u16; len as usize];
    if unsafe { GetCurrentPackageFamilyName(&mut len, Some(PWSTR(buf.as_mut_ptr()))) } != ERROR_SUCCESS {
        return None;
    }
    Some(super::from_wide(&buf))
});

/// Whether this process runs with package identity (installed from the MSIX).
pub fn is_packaged() -> bool {
    *PACKAGED
}

/// The package family name (`<Name>_<PublisherId>`); `None` when unpackaged.
pub fn package_family_name() -> Option<String> {
    FAMILY.clone()
}

/// Turns the packaged app's startup task on or off and returns whether it is on afterwards.
/// `DisabledByUser` (switched off in Task Manager or Settings › Apps › Startup) and
/// `DisabledByPolicy` stay off, `EnabledByPolicy` stays on: those are the user's or the admin's
/// call. Blocks on WinRT async operations; never call it on the main thread.
pub fn set_startup_task(on: bool) -> windows::core::Result<bool> {
    let task = StartupTask::GetAsync(&HSTRING::from(STARTUP_TASK))?.join()?;
    let state = task.State()?;
    let state = if on && state == StartupTaskState::Disabled {
        task.RequestEnableAsync()?.join()?
    } else if !on && state == StartupTaskState::Enabled {
        task.Disable()?;
        StartupTaskState::Disabled
    } else {
        state
    };
    Ok(state == StartupTaskState::Enabled || state == StartupTaskState::EnabledByPolicy)
}
