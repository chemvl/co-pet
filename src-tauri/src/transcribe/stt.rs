//! Speech to text: whisper.cpp in process (Metal) with whisper.cpp's Silero VAD.
//!
//! whisper.cpp applies `FullParams`' VAD only in `whisper_full` (the context's own state);
//! whisper-rs runs `whisper_full_with_state`, which ignores it. So the VAD runs here, the same way
//! whisper.cpp does it: speech stretches are spliced together with 100 ms of silence between
//! them, Whisper transcribes that, and segment times are mapped back to the track's timeline.
use std::ffi::{c_int, c_void};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use whisper_rs::whisper_rs_sys::{whisper_context, whisper_state};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperVadContext,
    WhisperVadContextParams, WhisperVadParams,
};

use crate::error::{AppError, AppResult};

/// Primes the decoder for Vietnamese meetings that mix in English terms.
const VI_EN_PROMPT: &str = "Cuộc họp dùng cả tiếng Việt và English, ví dụ: deadline, API, review, merge request.";
/// Whisper's input rate, in samples per ms.
const SAMPLES_PER_MS: i64 = 16;
/// Silence between spliced speech stretches (whisper.cpp uses the same).
const GAP_SAMPLES: usize = 1600;

/// A recognised stretch of speech on the track's timeline.
pub struct RawSeg {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

pub trait SpeechToText {
    /// Transcribes 16 kHz mono `samples` in `language` (`auto`, `vi+en` or a Whisper code).
    /// `progress` gets 0–1; setting `cancel` stops the run with `cancelled`. Returns the segments
    /// and the language Whisper used (detected for `auto`); no speech gives no segments and no
    /// language.
    fn transcribe(
        &mut self,
        samples: &[f32],
        language: &str,
        progress: &mut dyn FnMut(f32),
        cancel: &AtomicBool,
    ) -> AppResult<(Vec<RawSeg>, Option<String>)>;
}

pub struct WhisperStt {
    ctx: WhisperContext,
    vad: WhisperVadContext,
}

impl WhisperStt {
    /// Loads the Whisper model on the GPU and the VAD model. Either failing is `speechModel`.
    pub fn load(model: &Path, vad: &Path) -> AppResult<Self> {
        let mut params = WhisperContextParameters::default();
        params.use_gpu(true);
        let ctx = WhisperContext::new_with_params(model, params).map_err(|e| {
            eprintln!("[transcribe] whisper load {}: {e}", model.display());
            AppError::Code("speechModel")
        })?;
        let vad = WhisperVadContext::new(&vad.to_string_lossy(), WhisperVadContextParams::default()).map_err(|e| {
            eprintln!("[transcribe] VAD load {}: {e}", vad.display());
            AppError::Code("speechModel")
        })?;
        Ok(WhisperStt { ctx, vad })
    }
}

/// A speech stretch: where it starts on the track and in the spliced audio, and its length
/// (samples).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Stretch {
    orig: usize,
    spliced: usize,
    len: usize,
}

/// Copies the speech ranges `[start, end)` of `samples` back to back, `GAP_SAMPLES` of silence
/// between them.
fn splice(samples: &[f32], speech: &[(usize, usize)]) -> (Vec<f32>, Vec<Stretch>) {
    let mut out = Vec::new();
    let mut stretches = Vec::with_capacity(speech.len());
    for (i, &(start, end)) in speech.iter().enumerate() {
        if i > 0 {
            out.resize(out.len() + GAP_SAMPLES, 0.0);
        }
        stretches.push(Stretch { orig: start, spliced: out.len(), len: end - start });
        out.extend_from_slice(&samples[start..end]);
    }
    (out, stretches)
}

/// Which end of a segment a time is, for times that fall in the silence between stretches.
#[derive(Clone, Copy)]
enum Edge {
    Start,
    End,
}

/// Maps a time in the spliced audio back to the track. Inside a stretch it keeps its offset; in a
/// gap a start moves to the next stretch's start and an end to the previous stretch's end, so a
/// segment never spans silence that was cut; past either end it clamps to the speech.
fn to_track_ms(ms: i64, stretches: &[Stretch], edge: Edge) -> i64 {
    let t = ms * SAMPLES_PER_MS;
    let i = stretches.partition_point(|s| s.spliced as i64 <= t);
    let Some(s) = i.checked_sub(1).map(|i| stretches[i]) else {
        return stretches.first().map_or(ms, |s| s.orig as i64 / SAMPLES_PER_MS);
    };
    let offset = t - s.spliced as i64;
    let len = s.len as i64;
    let track = match (edge, stretches.get(i)) {
        // A start exactly at a stretch's end begins the next one.
        (Edge::Start, Some(next)) if offset >= len => next.orig as i64,
        // An end exactly at a later stretch's start ends the previous one.
        (Edge::End, _) if offset == 0 && i >= 2 => {
            let prev = stretches[i - 2];
            (prev.orig + prev.len) as i64
        }
        _ => s.orig as i64 + offset.min(len),
    };
    track / SAMPLES_PER_MS
}

/// whisper.cpp progress (percent) → the caller's `&mut dyn FnMut(f32)` behind `user`.
unsafe extern "C" fn on_progress(_: *mut whisper_context, _: *mut whisper_state, percent: c_int, user: *mut c_void) {
    let progress = &mut *(user as *mut &mut dyn FnMut(f32));
    progress(percent as f32 / 100.0);
}

/// Polled by whisper.cpp (possibly from its worker threads): the caller's `AtomicBool` behind
/// `user`.
unsafe extern "C" fn should_abort(user: *mut c_void) -> bool {
    (*(user as *const AtomicBool)).load(Ordering::Relaxed)
}

impl SpeechToText for WhisperStt {
    fn transcribe(
        &mut self,
        samples: &[f32],
        language: &str,
        mut progress: &mut dyn FnMut(f32),
        cancel: &AtomicBool,
    ) -> AppResult<(Vec<RawSeg>, Option<String>)> {
        let whisper_err = |e: whisper_rs::WhisperError| AppError::msg(format!("whisper: {e}"));
        if samples.is_empty() {
            return Ok((Vec::new(), None));
        }
        // VAD times are centiseconds (160 samples).
        let to_sample = |cs: f32| ((cs.max(0.0) * 160.0) as usize).min(samples.len());
        let speech: Vec<(usize, usize)> = self
            .vad
            .segments_from_samples(WhisperVadParams::default(), samples)
            .map_err(whisper_err)?
            .map(|s| (to_sample(s.start), to_sample(s.end)))
            .filter(|(start, end)| end > start)
            .collect();
        if speech.is_empty() {
            return Ok((Vec::new(), None));
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Code("cancelled"));
        }
        let (spliced, stretches) = splice(samples, &speech);

        let mut state = self.ctx.create_state().map_err(whisper_err)?;
        let mut params = FullParams::new(SamplingStrategy::BeamSearch { beam_size: 5, patience: -1.0 });
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(8);
        params.set_n_threads(threads as c_int);
        params.set_translate(false);
        params.set_no_context(true);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        let lang = match language {
            "vi+en" => {
                params.set_initial_prompt(VI_EN_PROMPT);
                "vi"
            }
            other => other,
        };
        params.set_language(Some(lang));
        // The `_safe` callback setters need `'static` closures; these borrow the caller's
        // `progress` and `cancel`, which outlive `state.full` (the only place whisper.cpp calls
        // them).
        unsafe {
            params.set_progress_callback(Some(on_progress));
            params.set_progress_callback_user_data(&mut progress as *mut &mut dyn FnMut(f32) as *mut c_void);
            params.set_abort_callback(Some(should_abort));
            params.set_abort_callback_user_data(cancel as *const AtomicBool as *mut c_void);
        }

        let result = state.full(params, &spliced);
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Code("cancelled"));
        }
        result.map_err(whisper_err)?;

        let segments = state
            .as_iter()
            .filter_map(|seg| {
                let text = seg.to_str_lossy().ok()?.trim().to_string();
                if text.is_empty() {
                    return None;
                }
                // Timestamps are in centiseconds.
                let start_ms = to_track_ms(seg.start_timestamp() * 10, &stretches, Edge::Start);
                let end_ms = to_track_ms(seg.end_timestamp() * 10, &stretches, Edge::End).max(start_ms);
                Some(RawSeg { start_ms, end_ms, text })
            })
            .collect();
        let detected = whisper_rs::get_lang_str(state.full_lang_id_from_state()).map(str::to_string);
        Ok((segments, detected))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_model_is_speech_model_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("ggml-missing.bin");
        let err = WhisperStt::load(&missing, &dir.path().join("vad.bin")).err().unwrap();
        assert!(matches!(err, AppError::Code("speechModel")), "{err}");
    }

    #[test]
    fn spliced_times_map_back_to_the_track() {
        // Speech at 10.0–12.0 s and 20.0–21.0 s of a 30 s track.
        let samples: Vec<f32> = (0..480_000).map(|i| i as f32).collect();
        let (spliced, stretches) = splice(&samples, &[(160_000, 192_000), (320_000, 336_000)]);
        assert_eq!(spliced.len(), 32_000 + GAP_SAMPLES + 16_000);
        assert_eq!((spliced[0], spliced[31_999]), (160_000.0, 191_999.0));
        assert!(spliced[32_000..32_000 + GAP_SAMPLES].iter().all(|&s| s == 0.0));
        assert_eq!(spliced[32_000 + GAP_SAMPLES], 320_000.0);

        let start = |ms| to_track_ms(ms, &stretches, Edge::Start);
        let end = |ms| to_track_ms(ms, &stretches, Edge::End);
        assert_eq!(start(0), 10_000, "the first stretch starts at 10 s");
        assert_eq!((start(1_500), end(1_500)), (11_500, 11_500), "inside a stretch both keep the offset");
        assert_eq!((start(2_000), end(2_000)), (20_000, 12_000), "a stretch's end starts the next speech");
        assert_eq!(start(2_050), 20_000, "a start in the gap moves to the next speech");
        assert_eq!(end(2_050), 12_000, "an end in the gap moves back to the previous speech");
        assert_eq!(start(2_100), 20_000, "the second stretch starts at 20 s");
        assert_eq!(end(2_100), 12_000, "an end at the next stretch's start stays in the previous speech");
        assert_eq!(end(2_600), 20_500);
        assert_eq!((start(9_999), end(9_999)), (21_000, 21_000), "past the end clamps to the speech");
    }
}
