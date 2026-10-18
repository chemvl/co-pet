//! D8: WASAPI loopback of the default output device through cpal (input stream on an output
//! device = `AUDCLNT_STREAMFLAGS_LOOPBACK`), averaged to mono and resampled to 16 kHz.
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{mpsc, Arc};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::transcribe::recorder::{mono, Chunk, TrackState};

/// The rate system audio is delivered at.
pub const RATE: u32 = 16_000;

/// A running loopback stream, owned until [`SystemStream::stop`].
pub struct SystemStream(cpal::Stream);

impl SystemStream {
    pub fn stop(self) {
        if let Err(e) = self.0.pause() {
            eprintln!("[recorder] stopping system audio: {e}");
        }
    }
}

/// Starts capturing what the default output device plays (the whole output mix). A fatal stream
/// error turns the track off, raises the returned flag, then calls `on_failure`; the mic
/// continues. Glitches and default-device reroutes are ignored, as for the mic.
pub fn build(
    chunks: &mpsc::Sender<Chunk>,
    state: &Arc<TrackState>,
    on_failure: impl Fn() + Send + Sync + 'static,
) -> Result<(SystemStream, Arc<AtomicBool>), String> {
    let device = cpal::default_host().default_output_device().ok_or("no output device")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let rate = supported.sample_rate();
    let channels = usize::from(supported.channels().max(1));
    let failed = Arc::new(AtomicBool::new(false));
    let (track, flag) = (state.clone(), failed.clone());
    let on_error = move |e: cpal::Error| match e.kind() {
        // Glitches and automatic reroutes: the stream keeps running.
        cpal::ErrorKind::Xrun | cpal::ErrorKind::DeviceChanged | cpal::ErrorKind::RealtimeDenied => {}
        _ => {
            eprintln!("[recorder] loopback stream: {e}");
            flag.store(true, Relaxed);
            track.on.store(false, Relaxed);
            on_failure();
        }
    };
    let config = supported.config();
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => {
            device.build_input_stream(config, mono::<f32>(chunks.clone(), channels, rate, RATE), on_error, None)
        }
        cpal::SampleFormat::I16 => {
            device.build_input_stream(config, mono::<i16>(chunks.clone(), channels, rate, RATE), on_error, None)
        }
        f => return Err(format!("loopback format {f:?}")),
    }
    .map_err(|e| e.to_string())?;
    // Before starting: a failure reported right away must win.
    state.on.store(true, Relaxed);
    stream.play().map_err(|e| e.to_string())?;
    Ok((SystemStream(stream), failed))
}
