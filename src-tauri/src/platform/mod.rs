//! OS integration. Everything that talks to AppKit, CoreGraphics or the pasteboard lives here so a
//! Windows implementation can be added beside `macos/` later.
#[cfg(target_os = "macos")]
pub mod macos;
