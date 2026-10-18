//! System audio capture through ScreenCaptureKit.
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::mpsc;
use std::sync::Arc;

use screencapturekit::prelude::{
    CMSampleBuffer, CMSampleBufferExt, ErrorHandler, SCContentFilter, SCShareableContent, SCStream,
    SCStreamConfiguration, SCStreamOutputType,
};

use crate::transcribe::recorder::{Chunk, TrackState};

/// The rate system audio is captured at.
pub const RATE: u32 = 16_000;

/// A running system audio stream, owned until [`SystemStream::stop`].
pub struct SystemStream(SCStream);

impl SystemStream {
    pub fn stop(self) {
        if let Err(e) = self.0.stop_capture() {
            eprintln!("[recorder] stopping system audio: {e}");
        }
    }
}

/// The `f32` samples of a ScreenCaptureKit audio buffer (mono Float32 PCM).
fn audio_samples(sample: &CMSampleBuffer) -> Option<Vec<f32>> {
    let list = sample.audio_buffer_list()?;
    let buf = list.get(0)?;
    Some(buf.data().chunks_exact(4).map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]])).collect())
}

/// Starts capturing the first display's audio (2×2 video, 16 kHz mono, Co-pet's own sound
/// excluded). A stream failure turns the track off, raises the returned flag, then calls
/// `on_failure`; the mic continues.
pub fn build(
    chunks: &mpsc::Sender<Chunk>,
    state: &Arc<TrackState>,
    on_failure: impl Fn() + Send + Sync + 'static,
) -> Result<(SystemStream, Arc<AtomicBool>), String> {
    let content = SCShareableContent::get().map_err(|e| e.to_string())?;
    let display = content.displays().into_iter().next().ok_or("no display")?;
    let filter = SCContentFilter::create().with_display(&display).with_excluding_windows(&[]).build();
    let config = SCStreamConfiguration::new()
        .with_width(2)
        .with_height(2)
        .with_captures_audio(true)
        .with_sample_rate(RATE as i32)
        .with_channel_count(1)
        .with_excludes_current_process_audio(true);
    let failed = Arc::new(AtomicBool::new(false));
    let (track, flag) = (state.clone(), failed.clone());
    let mut stream = SCStream::new_with_delegate(
        &filter,
        &config,
        ErrorHandler::new(move |e| {
            eprintln!("[recorder] system audio stopped: {e}");
            flag.store(true, Relaxed);
            track.on.store(false, Relaxed);
            on_failure();
        }),
    );
    let chunks = chunks.clone();
    stream
        .add_output_handler(
            move |sample: CMSampleBuffer, kind: SCStreamOutputType| {
                if kind == SCStreamOutputType::Audio {
                    if let Some(samples) = audio_samples(&sample) {
                        let _ = chunks.send(Chunk::Samples(samples));
                    }
                }
            },
            SCStreamOutputType::Audio,
        )
        .ok_or("audio output rejected")?;
    // Before starting: a failure reported right away must win.
    state.on.store(true, Relaxed);
    stream.start_capture().map_err(|e| e.to_string())?;
    Ok((SystemStream(stream), failed))
}
