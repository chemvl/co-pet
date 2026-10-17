//! Meeting transcription.
pub mod assemble;
pub mod audio;
pub mod diarize;
pub mod models;
pub mod recorder;
pub mod stt;
pub mod worker;

/// Crash recovery, the transcription worker and the audio retention loop (in setup, after
/// `sync::start`, with `TranscribeQueue` managed).
pub use worker::start;
