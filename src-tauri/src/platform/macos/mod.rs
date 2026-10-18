//! macOS integration. Same public names as `platform::win`; see `platform/mod.rs`. AppKit objects
//! are main-thread-only: callers off the main thread go through `platform::on_main` (async) or
//! `platform::on_main_sync` (blocking).
pub mod apps;
pub mod audio_convert;
pub mod calendar;
pub mod drop_target;
pub mod frontmost;
pub mod fullscreen;
pub mod idle;
pub mod keys;
pub mod locale;
pub mod mic_users;
pub mod ocr;
pub mod pasteboard;
pub mod permissions;
pub mod pet_window;
pub mod pointer;
pub mod process;
pub mod screens;
pub mod screenshot;
pub mod secret_store;
pub mod system_audio;
pub mod window_titles;

pub type MainThread = objc2::MainThreadMarker;
