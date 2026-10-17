//! The meeting recorder (managed state, at most one session): two tracks under
//! `recordings/<meetingId>/` — `mic-raw.wav` at the input device's rate and `system.wav`
//! (ScreenCaptureKit) at 16 kHz — both aligned to one clock that excludes pauses.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::sync::mpsc::{self, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use chrono::{Local, TimeZone};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use screencapturekit::prelude::{
    CMSampleBuffer, CMSampleBufferExt, ErrorHandler, SCContentFilter, SCShareableContent, SCStream,
    SCStreamConfiguration, SCStreamOutputType,
};
use tauri::{AppHandle, Manager};
use tokio::sync::oneshot;
use tokio::time::MissedTickBehavior;

use super::audio::{linear_resample, TrackWriter};
use crate::db::{meetings, now_ms, settings, Db};
use crate::error::{AppError, AppResult};
use crate::events::{self, RecordingLevel};
use crate::export;
use crate::i18n::{self, short_date, tr, Lang};
use crate::model::RecordingStatus;
use crate::paths::Paths;
use crate::platform::macos::permissions;

const MIC_FILE: &str = "mic-raw.wav";
const SYSTEM_FILE: &str = "system.wav";
const SYSTEM_RATE: u32 = 16_000;
/// Active time after which a recording stops by itself.
const MAX_ACTIVE: Duration = Duration::from_secs(4 * 3600);
/// Shorter recordings are discarded.
const MIN_DURATION_MS: i64 = 2_000;
const LEVEL_TICK: Duration = Duration::from_millis(100);
/// How often the mic thread checks for a failed stream.
const MIC_POLL: Duration = Duration::from_millis(500);
/// A track that delivered nothing for this long reads level 0.
const LEVEL_STALE: Duration = Duration::from_millis(100);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------- clock ----------

/// Recording time: wall time since the start minus pauses.
struct Clock {
    start: Instant,
    paused: Duration,
    paused_at: Option<Instant>,
}

impl Clock {
    fn new() -> Self {
        Clock { start: Instant::now(), paused: Duration::ZERO, paused_at: None }
    }

    fn active(&self) -> Duration {
        let now = Instant::now();
        let paused = self.paused + self.paused_at.map_or(Duration::ZERO, |p| now - p);
        (now - self.start).saturating_sub(paused)
    }

    fn is_paused(&self) -> bool {
        self.paused_at.is_some()
    }

    fn pause(&mut self) {
        self.paused_at.get_or_insert_with(Instant::now);
    }

    fn resume(&mut self) {
        if let Some(p) = self.paused_at.take() {
            self.paused += p.elapsed();
        }
    }
}

type SharedClock = Arc<Mutex<Clock>>;

// ---------- tracks ----------

/// What a capture callback sends its writer thread.
enum Chunk {
    Samples(Vec<f32>),
    /// Capture stopped: finish the file.
    End,
}

/// What a track's threads share with the recorder.
#[derive(Default)]
struct TrackState {
    /// Capturing right now.
    on: AtomicBool,
    /// `TrackWriter::level` as `f32` bits.
    level: AtomicU32,
}

impl TrackState {
    fn level(&self) -> f32 {
        if self.on.load(Relaxed) {
            f32::from_bits(self.level.load(Relaxed))
        } else {
            0.0
        }
    }
}

/// A capture thread: owns its stream, sends `Chunk::Samples` to the writer, reports the rate it
/// records at through `ready` once the stream runs, and returns when `stop` receives (or its
/// sender is dropped).
type Owner = fn(AppHandle, mpsc::Sender<Chunk>, mpsc::Receiver<()>, Arc<TrackState>, oneshot::Sender<Result<u32, String>>);

/// One running track: the thread owning the capture stream and the thread writing its file.
/// Dropping it without `finish` still ends both threads (the file is finalized, not joined).
struct Track {
    rate: u32,
    state: Arc<TrackState>,
    stop: mpsc::Sender<()>,
    owner: Option<JoinHandle<()>>,
    chunks: mpsc::Sender<Chunk>,
    writer: Option<JoinHandle<u64>>,
}

impl Track {
    /// Stops capture and finishes the file (blocking); returns its length in ms.
    fn finish(mut self) -> i64 {
        let _ = self.stop.send(());
        if self.owner.take().is_some_and(|h| h.join().is_err()) {
            eprintln!("[recorder] capture thread panicked");
        }
        let _ = self.chunks.send(Chunk::End);
        let samples = self.writer.take().map_or(0, |h| {
            h.join().unwrap_or_else(|_| {
                eprintln!("[recorder] writer thread panicked");
                0
            })
        });
        (samples * 1000 / u64::from(self.rate)) as i64
    }
}

impl Drop for Track {
    /// Non-blocking: the writer holds a `stop` sender, so the threads only end when told to.
    fn drop(&mut self) {
        let _ = self.stop.send(());
        let _ = self.chunks.send(Chunk::End);
    }
}

/// Starts `owner` on its own thread and, once its stream runs, a writer for `path` at the rate it
/// reports. `Ok(None)` when the stream could not start.
async fn start_track(
    app: &AppHandle,
    name: &'static str,
    owner: Owner,
    path: PathBuf,
    clock: &SharedClock,
) -> AppResult<Option<Track>> {
    let (chunks, chunks_rx) = mpsc::channel();
    let (stop, stop_rx) = mpsc::channel();
    let (ready, ready_rx) = oneshot::channel();
    let state = Arc::new(TrackState::default());
    let owner = {
        let (app, chunks, state) = (app.clone(), chunks.clone(), state.clone());
        thread::Builder::new()
            .name(format!("copet-rec-{name}"))
            .spawn(move || owner(app, chunks, stop_rx, state, ready))?
    };
    let rate = match ready_rx.await {
        Ok(Ok(rate)) => rate,
        Ok(Err(e)) => {
            eprintln!("[recorder] {name} unavailable: {e}");
            return Ok(None);
        }
        Err(_) => {
            eprintln!("[recorder] {name} capture thread ended before starting");
            return Ok(None);
        }
    };
    // From here an early return drops `stop`, which ends the capture thread.
    let writer = TrackWriter::create(&path, rate)?;
    let writer = spawn_writer(app.clone(), name, writer, chunks_rx, clock.clone(), state.clone(), stop.clone())?;
    Ok(Some(Track { rate, state, stop, owner: Some(owner), chunks, writer: Some(writer) }))
}

/// Writes chunks until `End`, dropping them while paused, and publishes the level (0 after
/// 100 ms without audio). A write failure turns the track off and stops its capture thread.
/// Returns the samples written.
fn spawn_writer(
    app: AppHandle,
    name: &'static str,
    mut writer: TrackWriter,
    chunks: mpsc::Receiver<Chunk>,
    clock: SharedClock,
    state: Arc<TrackState>,
    stop: mpsc::Sender<()>,
) -> AppResult<JoinHandle<u64>> {
    let handle = thread::Builder::new().name(format!("copet-rec-{name}-writer")).spawn(move || {
        loop {
            match chunks.recv_timeout(LEVEL_STALE) {
                Ok(Chunk::Samples(samples)) => {
                    let active = {
                        let clock = lock(&clock);
                        if clock.is_paused() {
                            continue;
                        }
                        clock.active()
                    };
                    if let Err(e) = writer.push(&samples, active) {
                        eprintln!("[recorder] {name}: write failed: {e}");
                        // `stop` first: the mic thread checks it before turning the track on.
                        let _ = stop.send(());
                        state.on.store(false, Relaxed);
                        state.level.store(0f32.to_bits(), Relaxed);
                        emit_status(&app);
                        break;
                    }
                    state.level.store(writer.level().to_bits(), Relaxed);
                }
                Ok(Chunk::End) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => state.level.store(0f32.to_bits(), Relaxed),
            }
        }
        let written = writer.written();
        if let Err(e) = writer.finish() {
            eprintln!("[recorder] {name}: finishing the file failed: {e}");
        }
        written
    })?;
    Ok(handle)
}

// ---------- mic ----------

/// The input callback: frames averaged to mono, resampled from `from` to `to` Hz. Each resampled
/// buffer is trimmed or extended by a sample so the total stays `input × to / from` (per-buffer
/// rounding would otherwise drift the track off the clock).
fn mono<T>(
    chunks: mpsc::Sender<Chunk>,
    channels: usize,
    from: u32,
    to: u32,
) -> impl FnMut(&[T], &cpal::InputCallbackInfo) + Send + 'static
where
    T: SizedSample + 'static,
    f32: FromSample<T>,
{
    let (mut fed, mut made) = (0u64, 0u64);
    move |data: &[T], _: &cpal::InputCallbackInfo| {
        let mono: Vec<f32> = data
            .chunks(channels)
            .map(|frame| frame.iter().map(|&s| f32::from_sample_(s)).sum::<f32>() / frame.len() as f32)
            .collect();
        let mono = if from == to {
            mono
        } else {
            fed += mono.len() as u64;
            let want = fed * u64::from(to) / u64::from(from) - made;
            made += want;
            let mut out = linear_resample(&mono, from, to);
            out.resize(want as usize, out.last().copied().unwrap_or(0.0));
            out
        };
        let _ = chunks.send(Chunk::Samples(mono));
    }
}

/// Opens the current default input as mono chunks at `to` Hz (default: the device rate).
/// Returns the playing stream, the device rate and the flag its error callback raises when the
/// stream must be rebuilt.
fn build_mic(chunks: &mpsc::Sender<Chunk>, to: Option<u32>) -> Result<(cpal::Stream, u32, Arc<AtomicBool>), String> {
    let device = cpal::default_host().default_input_device().ok_or("no input device")?;
    let supported = device.default_input_config().map_err(|e| e.to_string())?;
    let rate = supported.sample_rate();
    let to = to.unwrap_or(rate);
    let channels = usize::from(supported.channels().max(1));
    let failed = Arc::new(AtomicBool::new(false));
    let flag = failed.clone();
    let on_error = move |e: cpal::Error| match e.kind() {
        // Glitches and automatic reroutes: the stream keeps running.
        cpal::ErrorKind::Xrun | cpal::ErrorKind::DeviceChanged | cpal::ErrorKind::RealtimeDenied => {}
        _ => {
            eprintln!("[recorder] mic stream: {e}");
            flag.store(true, Relaxed);
        }
    };
    let config = supported.config();
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => {
            device.build_input_stream(config, mono::<f32>(chunks.clone(), channels, rate, to), on_error, None)
        }
        cpal::SampleFormat::I16 => {
            device.build_input_stream(config, mono::<i16>(chunks.clone(), channels, rate, to), on_error, None)
        }
        cpal::SampleFormat::U16 => {
            device.build_input_stream(config, mono::<u16>(chunks.clone(), channels, rate, to), on_error, None)
        }
        other => return Err(format!("unsupported sample format {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, rate, failed))
}

/// Owns the `cpal` stream (not `Send`). After a stream error it rebuilds on the current default
/// input every 500 ms, converting to the first stream's rate.
fn mic_owner(
    app: AppHandle,
    chunks: mpsc::Sender<Chunk>,
    stop: mpsc::Receiver<()>,
    state: Arc<TrackState>,
    ready: oneshot::Sender<Result<u32, String>>,
) {
    let (stream, rate, failed) = match build_mic(&chunks, None) {
        Ok(v) => v,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    state.on.store(true, Relaxed);
    let _ = ready.send(Ok(rate));
    let mut current = Some((stream, failed));
    while let Err(RecvTimeoutError::Timeout) = stop.recv_timeout(MIC_POLL) {
        if current.as_ref().is_some_and(|(_, failed)| !failed.load(Relaxed)) {
            continue;
        }
        // Close the failed stream before opening the current default input.
        drop(current.take());
        current = match build_mic(&chunks, Some(rate)) {
            Ok((stream, _, failed)) => Some((stream, failed)),
            Err(e) => {
                if state.on.load(Relaxed) {
                    eprintln!("[recorder] mic unavailable: {e}");
                }
                None
            }
        };
        // Stopped meanwhile (e.g. the writer failed): never turn the track back on.
        if !matches!(stop.try_recv(), Err(TryRecvError::Empty)) {
            break;
        }
        let on = current.is_some();
        if state.on.swap(on, Relaxed) != on {
            emit_status(&app);
        }
    }
}

// ---------- system audio ----------

/// The `f32` samples of a ScreenCaptureKit audio buffer (mono Float32 PCM).
fn audio_samples(sample: &CMSampleBuffer) -> Option<Vec<f32>> {
    let list = sample.audio_buffer_list()?;
    let buf = list.get(0)?;
    Some(buf.data().chunks_exact(4).map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]])).collect())
}

/// Starts capturing the first display's audio (2×2 video, 16 kHz mono, Co-pet's own sound
/// excluded). A stream failure turns the track off and raises the returned flag; the mic
/// continues.
fn build_system(
    app: &AppHandle,
    chunks: &mpsc::Sender<Chunk>,
    state: &Arc<TrackState>,
) -> Result<(SCStream, Arc<AtomicBool>), String> {
    let content = SCShareableContent::get().map_err(|e| e.to_string())?;
    let display = content.displays().into_iter().next().ok_or("no display")?;
    let filter = SCContentFilter::create().with_display(&display).with_excluding_windows(&[]).build();
    let config = SCStreamConfiguration::new()
        .with_width(2)
        .with_height(2)
        .with_captures_audio(true)
        .with_sample_rate(SYSTEM_RATE as i32)
        .with_channel_count(1)
        .with_excludes_current_process_audio(true);
    let failed = Arc::new(AtomicBool::new(false));
    let (app, track, flag) = (app.clone(), state.clone(), failed.clone());
    let mut stream = SCStream::new_with_delegate(
        &filter,
        &config,
        ErrorHandler::new(move |e| {
            eprintln!("[recorder] system audio stopped: {e}");
            flag.store(true, Relaxed);
            track.on.store(false, Relaxed);
            emit_status(&app);
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
    Ok((stream, failed))
}

/// Owns the `SCStream` until `stop`.
fn system_owner(
    app: AppHandle,
    chunks: mpsc::Sender<Chunk>,
    stop: mpsc::Receiver<()>,
    state: Arc<TrackState>,
    ready: oneshot::Sender<Result<u32, String>>,
) {
    let (stream, failed) = match build_system(&app, &chunks, &state) {
        Ok(v) => v,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let _ = ready.send(Ok(SYSTEM_RATE));
    let _ = stop.recv();
    // A failed stream has stopped already.
    if !failed.load(Relaxed) {
        if let Err(e) = stream.stop_capture() {
            eprintln!("[recorder] stopping system audio: {e}");
        }
    }
}

// ---------- recorder ----------

struct Session {
    meeting_id: String,
    started_at: i64,
    language: String,
    dir: PathBuf,
    clock: SharedClock,
    mic: Option<Track>,
    system: Option<Track>,
    levels: tauri::async_runtime::JoinHandle<()>,
}

impl Session {
    fn status(&self) -> RecordingStatus {
        let clock = lock(&self.clock);
        let on = |t: &Option<Track>| t.as_ref().is_some_and(|t| t.state.on.load(Relaxed));
        RecordingStatus {
            state: if clock.is_paused() { "paused" } else { "recording" }.into(),
            meeting_id: Some(self.meeting_id.clone()),
            started_at: Some(self.started_at),
            elapsed_ms: clock.active().as_millis() as i64,
            language: self.language.clone(),
            mic: on(&self.mic),
            system: on(&self.system),
            ended_reason: None,
        }
    }

    /// Stops both tracks and finishes their files (blocking: joins the threads). Returns the
    /// longer track's length in ms and whether each track holds audio.
    fn finish(self) -> (i64, bool, bool) {
        self.levels.abort();
        let mic = self.mic.map_or(0, Track::finish);
        let system = self.system.map_or(0, Track::finish);
        (mic.max(system), mic > 0, system > 0)
    }
}

fn idle(ended_reason: Option<&str>) -> RecordingStatus {
    RecordingStatus {
        state: "idle".into(),
        meeting_id: None,
        started_at: None,
        elapsed_ms: 0,
        language: "auto".into(),
        mic: false,
        system: false,
        ended_reason: ended_reason.map(String::from),
    }
}

/// Emits the current status (capture threads call this when a track turns on or off).
fn emit_status(app: &AppHandle) {
    if let Some(r) = app.try_state::<Recorder>() {
        events::recording_status(app, r.status());
    }
}

/// `Meeting · Fri 17 Oct 14:05` / `Cuộc họp · T6, 17/10 14:05`.
fn default_title(lang: Lang, started_at: i64) -> String {
    let t = Local.timestamp_millis_opt(started_at).single().unwrap_or_default();
    format!("{} · {} {}", tr(lang, "meeting.defaultTitle"), short_date(lang, t.date_naive()), t.format("%H:%M"))
}

/// Deletes a recording's folder and its meeting row.
fn discard(app: &AppHandle, meeting_id: &str, dir: &Path) {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            eprintln!("[recorder] deleting {}: {e}", dir.display())
        }
        _ => {}
    }
    if let Err(e) = meetings::soft_delete(&app.state::<Db>().conn(), meeting_id) {
        eprintln!("[recorder] deleting meeting {meeting_id}: {e}");
    }
    meetings::notify_changed(app, vec![meeting_id.to_string()]);
}

/// Starts the mic and/or system track in `dir`; at least one must run.
async fn start_tracks(
    app: &AppHandle,
    dir: &Path,
    clock: &SharedClock,
    mic: bool,
    system: bool,
) -> AppResult<(Option<Track>, Option<Track>)> {
    std::fs::create_dir_all(dir)?;
    let (m, s) = tokio::join!(
        async {
            if mic {
                start_track(app, "mic", mic_owner, dir.join(MIC_FILE), clock).await
            } else {
                Ok(None)
            }
        },
        async {
            if system {
                start_track(app, "system", system_owner, dir.join(SYSTEM_FILE), clock).await
            } else {
                Ok(None)
            }
        },
    );
    match (m?, s?) {
        (None, None) => Err(AppError::Code(if mic { "micUnavailable" } else { "noAudioSource" })),
        tracks => Ok(tracks),
    }
}

/// Every 100 ms while recording: `recording-level` to the pet. At 4 h of active time it stops
/// the recording (`maxLength`).
fn spawn_levels(
    app: AppHandle,
    meeting_id: String,
    clock: SharedClock,
    mic: Option<Arc<TrackState>>,
    system: Option<Arc<TrackState>>,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(LEVEL_TICK);
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let level = |t: &Option<Arc<TrackState>>| t.as_ref().map_or(0.0, |t| t.level());
        loop {
            tick.tick().await;
            let (paused, active) = {
                let clock = lock(&clock);
                (clock.is_paused(), clock.active())
            };
            if paused {
                continue;
            }
            if active >= MAX_ACTIVE {
                // A task of its own: stopping aborts this one.
                tauri::async_runtime::spawn(async move {
                    if let Some(r) = app.try_state::<Recorder>() {
                        if let Err(e) = r.end(&app, Some(&meeting_id), Some("maxLength")).await {
                            eprintln!("[recorder] stop at max length: {e}");
                        }
                    }
                });
                return;
            }
            events::recording_level(&app, RecordingLevel { mic: level(&mic), system: level(&system) });
        }
    })
}

/// Managed state (`app.state::<Recorder>()`).
#[derive(Default)]
pub struct Recorder {
    /// Serializes `start`, `stop` and `abort`.
    op: tokio::sync::Mutex<()>,
    session: Mutex<Option<Session>>,
}

impl Recorder {
    /// Starts a recording in `language` (default `transcribe.language`): asks for the
    /// microphone while undecided, creates the meeting row and its folder, starts every track
    /// available. Errors: `alreadyRecording`, `invalid`, `noAudioSource`, `micUnavailable`.
    #[allow(dead_code)] // `start_recording` command (Task 3.4)
    pub async fn start(&self, app: &AppHandle, language: Option<String>) -> AppResult<RecordingStatus> {
        let _op = self.op.lock().await;
        if self.is_active() {
            return Err(AppError::Code("alreadyRecording"));
        }
        let language = match language {
            Some(l) => l,
            None => settings::read(app, "transcribe.language")?,
        };
        if !meetings::LANGUAGES.contains(&language.as_str()) {
            return Err(AppError::Code("invalid"));
        }
        let mic_granted = match permissions::microphone_status() {
            "granted" => true,
            "notDetermined" => permissions::request_microphone().await,
            _ => false,
        };
        let system = permissions::screen_recording_granted();
        if !mic_granted && !system {
            return Err(AppError::Code("noAudioSource"));
        }
        let mic = mic_granted && cpal::default_host().default_input_device().is_some();
        if !mic && !system {
            return Err(AppError::Code("micUnavailable"));
        }

        let started_at = now_ms();
        let title = default_title(i18n::lang(app), started_at);
        let meeting_id = {
            let conn = app.state::<Db>().inner().conn();
            let device_id = export::device_id(&conn)?;
            meetings::create(&conn, "recording", &language, &device_id, started_at, &title, None)?
        };
        let dir = app.state::<Paths>().recording_dir(&meeting_id);
        let clock: SharedClock = Arc::new(Mutex::new(Clock::new()));
        let (mic, system) = match start_tracks(app, &dir, &clock, mic, system).await {
            Ok(tracks) => tracks,
            Err(e) => {
                discard(app, &meeting_id, &dir);
                return Err(e);
            }
        };
        let levels = spawn_levels(
            app.clone(),
            meeting_id.clone(),
            clock.clone(),
            mic.as_ref().map(|t| t.state.clone()),
            system.as_ref().map(|t| t.state.clone()),
        );
        let session = Session { meeting_id, started_at, language, dir, clock, mic, system, levels };
        let status = session.status();
        *lock(&self.session) = Some(session);
        events::recording_status(app, status.clone());
        Ok(status)
    }

    /// Applies `f` to the session and emits the new status; `notRecording` without one.
    fn update(&self, app: &AppHandle, f: impl FnOnce(&mut Session)) -> AppResult<RecordingStatus> {
        let status = {
            let mut slot = lock(&self.session);
            let session = slot.as_mut().ok_or(AppError::Code("notRecording"))?;
            f(session);
            session.status()
        };
        events::recording_status(app, status.clone());
        Ok(status)
    }

    /// While paused the clock stands still and incoming audio is dropped.
    #[allow(dead_code)] // `pause_recording` command (Task 3.4)
    pub fn pause(&self, app: &AppHandle) -> AppResult<RecordingStatus> {
        self.update(app, |s| lock(&s.clock).pause())
    }

    #[allow(dead_code)] // `resume_recording` command (Task 3.4)
    pub fn resume(&self, app: &AppHandle) -> AppResult<RecordingStatus> {
        self.update(app, |s| lock(&s.clock).resume())
    }

    /// Stops the recording and queues the meeting for transcription; returns its id. Under 2 s
    /// the recording is discarded (`tooShort`).
    #[allow(dead_code)] // `stop_recording` command and the quit guard (Task 3.4)
    pub async fn stop(&self, app: &AppHandle) -> AppResult<String> {
        self.end(app, None, None).await
    }

    /// `stop` for the session of `only` (any session when `None`); `ended_reason` goes into the
    /// idle status.
    async fn end(&self, app: &AppHandle, only: Option<&str>, ended_reason: Option<&str>) -> AppResult<String> {
        let _op = self.op.lock().await;
        let session = {
            let mut slot = lock(&self.session);
            match slot.take() {
                Some(s) if only.is_none_or(|id| id == s.meeting_id) => s,
                other => {
                    *slot = other;
                    return Err(AppError::Code("notRecording"));
                }
            }
        };
        let (id, dir) = (session.meeting_id.clone(), session.dir.clone());
        let (duration_ms, has_mic, has_system) = tauri::async_runtime::spawn_blocking(move || session.finish())
            .await
            .map_err(|e| AppError::msg(e.to_string()))?;
        if duration_ms < MIN_DURATION_MS {
            discard(app, &id, &dir);
            events::recording_status(app, idle(Some("tooShort")));
            return Err(AppError::Code("tooShort"));
        }
        let saved = {
            let conn = app.state::<Db>().inner().conn();
            meetings::set_duration(&conn, &id, duration_ms, has_mic, has_system)
                .and_then(|()| meetings::set_state(&conn, &id, "queued", None))
        };
        events::recording_status(app, idle(ended_reason));
        saved?;
        meetings::notify_changed(app, vec![id.clone()]);
        Ok(id)
    }

    /// Also updates the meeting row. Errors: `notRecording`, `invalid`.
    #[allow(dead_code)] // `set_recording_language` command (Task 3.4)
    pub fn set_language(&self, app: &AppHandle, language: String) -> AppResult<RecordingStatus> {
        let id = lock(&self.session).as_ref().map(|s| s.meeting_id.clone()).ok_or(AppError::Code("notRecording"))?;
        meetings::set_language(&app.state::<Db>().conn(), &id, &language)?;
        self.update(app, |s| s.language = language)
    }

    pub fn status(&self) -> RecordingStatus {
        lock(&self.session).as_ref().map_or_else(|| idle(None), Session::status)
    }

    pub fn is_active(&self) -> bool {
        lock(&self.session).is_some()
    }

    /// Delete everything: stops capture and deletes the recording's folder. The meeting row is
    /// left to the caller, which deletes the database.
    pub async fn abort(&self) {
        let _op = self.op.lock().await;
        let session = lock(&self.session).take();
        let Some(session) = session else { return };
        let dir = session.dir.clone();
        if let Err(e) = tauri::async_runtime::spawn_blocking(move || session.finish()).await {
            eprintln!("[recorder] abort: {e}");
        }
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            eprintln!("[recorder] abort: deleting {}: {e}", dir.display());
        }
    }
}
