//! Meeting transcription.
pub mod assemble;
pub mod audio;
pub mod calls;
pub mod diarize;
pub mod extract;
pub mod models;
pub mod recorder;
pub mod stt;
pub mod summary;
pub mod wer;
pub mod worker;

use tauri::{AppHandle, Manager};

/// Crash recovery, the transcription worker, the audio retention loop and call detection (in
/// setup, after `sync::start`, with `TranscribeQueue` and `Recorder` managed).
pub fn start(app: &AppHandle) {
    app.manage(calls::CallWatch::default());
    worker::start(app);
    calls::start(app);
}
