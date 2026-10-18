//! Meeting transcription.
pub mod assemble;
pub mod audio;
#[allow(dead_code)] // consumed by the call watcher runtime, the next task
pub mod calls;
pub mod diarize;
pub mod extract;
pub mod models;
pub mod recorder;
pub mod stt;
pub mod summary;
pub mod wer;
pub mod worker;

/// Crash recovery, the transcription worker and the audio retention loop (in setup, after
/// `sync::start`, with `TranscribeQueue` managed).
pub use worker::start;
