//! Meeting transcription.
pub mod audio;
#[allow(dead_code)] // run by the transcription worker (Task 4.4) and the benchmark (Task 4.5)
pub mod diarize;
pub mod models;
pub mod recorder;
#[allow(dead_code)] // run by the transcription worker (Task 4.4) and the benchmark (Task 4.5)
pub mod stt;
