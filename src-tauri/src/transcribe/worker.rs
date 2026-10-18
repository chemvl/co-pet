//! The transcription worker: one meeting at a time, only this device's, sleeping on a `Notify`
//! while nothing is queued or a recording holds it. Also startup crash recovery, imports, the
//! audio retention sweep and the meeting commands' logic.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;
use tauri::{AppHandle, Listener, Manager};
use tokio::sync::Notify;

use super::assemble::{assemble, langs_of};
use super::audio::{self, Convert, NativeConvert, TrackWriter};
use super::diarize::{Diarizer, SherpaDiarizer};
use super::models::{self, Pack, SPEAKER, SPEECH};
use super::recorder::Recorder;
use super::stt::{RawSeg, SpeechToText, WhisperStt};
use super::summary;
use crate::db::items::{self, MeetingItem};
use crate::db::meetings::{self, MeetingRow};
use crate::db::{now_ms, settings, Db};
use crate::error::{AppError, AppResult};
use crate::events::{self, PetReaction};
use crate::export;
use crate::i18n::{self, Lang};
use crate::llm::models::model_path;
use crate::model::{Actor, MeetingDetail, MeetingSummary};
use crate::paths::Paths;

/// The speech engines' rate.
const RATE: u32 = 16_000;
/// Shorter interrupted recordings are deleted at startup (as `Recorder::stop` does).
const MIN_RECOVERED_MS: i64 = 2_000;
const RETENTION_SETTING: &str = "transcribe.audioRetentionDays";
const RETENTION_EVERY: Duration = Duration::from_secs(6 * 3600);
const DAY_MS: i64 = 86_400_000;
/// `meetings-changed` for progress: at most once per tick, for steps of at least 1 %.
const PROGRESS_TICK: Duration = Duration::from_secs(1);
const PROGRESS_STEP: f64 = 0.01;
/// After a queue error, the worker retries this much later (or when poked).
const RETRY_AFTER: Duration = Duration::from_secs(60);
const NEEDS_MODEL: &str = "needsSpeechModel";
/// Job errors stored as they are; anything else is `transcribeFailed`.
const JOB_ERRORS: [&str; 4] = ["audioConvert", "unsupportedAudio", "speechModel", "noAudio"];

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Managed state (`app.state::<TranscribeQueue>()`). Progress lives only here: meetings are
/// synced rows, so it is never written to SQLite.
#[derive(Default)]
pub struct TranscribeQueue {
    notify: Notify,
    cancel: Arc<AtomicBool>,
    /// The meeting being transcribed.
    current: Mutex<Option<String>>,
    /// The meeting being transcribed and its fraction done.
    progress: Mutex<Option<(String, f64)>>,
    /// A recording is running: no transcription starts.
    held: AtomicBool,
    /// The models folder is moving: no transcription or summary starts.
    models_moving: AtomicBool,
}

impl TranscribeQueue {
    /// Wakes the worker.
    pub fn poke(&self) {
        self.notify.notify_one();
    }

    /// Cancels the running transcription; its meeting goes back to `queued`.
    pub fn cancel_current(&self) {
        if lock(&self.current).is_some() {
            self.cancel.store(true, SeqCst);
        }
    }

    /// [`Self::cancel_current`] when meeting `id` is the one running.
    pub fn cancel_if(&self, id: &str) {
        if lock(&self.current).as_deref() == Some(id) {
            self.cancel.store(true, SeqCst);
        }
    }

    /// No transcription runs while recording; a running one goes back to `queued`.
    pub fn pause_for_recording(&self) {
        self.held.store(true, SeqCst);
        self.cancel_current();
    }

    pub fn resume_after_recording(&self) {
        self.held.store(false, SeqCst);
    }

    /// Holds the queue for a models-folder move. `false` while a transcription runs (checked
    /// under the `current` lock, so `begin` either sees the flag or this sees the job).
    pub fn hold_for_models_move(&self) -> bool {
        let current = lock(&self.current);
        if current.is_some() {
            return false;
        }
        self.models_moving.store(true, SeqCst);
        true
    }

    /// Ends [`Self::hold_for_models_move`] and wakes the worker.
    pub fn end_models_move(&self) {
        self.models_moving.store(false, SeqCst);
        self.poke();
    }

    /// The running transcription's meeting id and fraction done.
    pub fn progress(&self) -> Option<(String, f64)> {
        lock(&self.progress).clone()
    }

    /// Marks `id` as running unless a recording or a models move holds the queue (checked under
    /// the `current` lock, so `pause_for_recording` either sees the job or the job sees `held`).
    fn begin(&self, id: &str) -> bool {
        let mut current = lock(&self.current);
        if self.held.load(SeqCst) || self.models_moving.load(SeqCst) {
            return false;
        }
        self.cancel.store(false, SeqCst);
        *current = Some(id.to_string());
        *lock(&self.progress) = Some((id.to_string(), 0.0));
        true
    }

    fn end(&self) {
        let mut current = lock(&self.current);
        *current = None;
        *lock(&self.progress) = None;
    }

    fn set_progress(&self, p: f64) {
        if let Some((_, v)) = lock(&self.progress).as_mut() {
            *v = p;
        }
    }

    fn cancelled(&self) -> AppResult<()> {
        if self.cancel.load(SeqCst) {
            return Err(AppError::Code("cancelled"));
        }
        Ok(())
    }
}

/// Wakes the worker (no-op before the queue is managed).
pub fn poke(app: &AppHandle) {
    if let Some(q) = app.try_state::<TranscribeQueue>() {
        q.poke();
    }
}

/// An audio or video MIME type (files Co-pet offers to transcribe).
pub fn is_media(mime: &str) -> bool {
    mime.starts_with("audio/") || mime.starts_with("video/")
}

// ---------- the job ----------

/// The engines a job runs: the real ones in the app, fakes in tests.
pub(crate) trait Engines {
    fn convert(&self) -> &dyn Convert;
    /// Loads speech to text; `speechModel` when it can't.
    fn speech(&self, paths: &Paths) -> AppResult<Box<dyn SpeechToText>>;
    /// Loads diarization; `speakerModel` when it can't.
    fn speaker(&self, paths: &Paths) -> AppResult<Box<dyn Diarizer>>;
}

/// The native audio converter, whisper.cpp and sherpa-onnx with the downloaded packs.
struct Real;

impl Engines for Real {
    fn convert(&self) -> &dyn Convert {
        &NativeConvert
    }

    fn speech(&self, paths: &Paths) -> AppResult<Box<dyn SpeechToText>> {
        Ok(Box::new(WhisperStt::load(&model_path(paths, &SPEECH[0]), &model_path(paths, &SPEECH[1]))?))
    }

    fn speaker(&self, paths: &Paths) -> AppResult<Box<dyn Diarizer>> {
        Ok(Box::new(SherpaDiarizer::load(&model_path(paths, &SPEAKER[0]), &model_path(paths, &SPEAKER[1]))?))
    }
}

/// What a job reads and writes.
pub(crate) struct Env<'a> {
    pub db: &'a Db,
    pub paths: &'a Paths,
    pub queue: &'a TranscribeQueue,
    /// For the meeting item's Markdown.
    pub lang: Lang,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Outcome {
    /// Still `queued` with `needsSpeechModel`.
    NeedsModel,
    /// Back to `queued`.
    Cancelled,
    /// `failed` with this error.
    Failed(&'static str),
    /// `done`; the meeting item's id.
    Done { item_id: String },
}

/// Keeps a queued meeting waiting for the speech pack. True when the row changed.
fn mark_needs_model(conn: &Connection, id: &str) -> AppResult<bool> {
    if meetings::get(conn, id)?.error.as_deref() == Some(NEEDS_MODEL) {
        return Ok(false);
    }
    meetings::set_state(conn, id, "queued", Some(NEEDS_MODEL))?;
    Ok(true)
}

/// Transcribes meeting `id` (blocking). `started` runs once the meeting is `transcribing`.
pub(crate) fn run_job(env: &Env, id: &str, engines: &dyn Engines, started: &dyn Fn()) -> Outcome {
    if !models::ready(env.paths, Pack::Speech) {
        if let Err(e) = mark_needs_model(&env.db.conn(), id) {
            eprintln!("[transcribe] {id}: {e}");
        }
        return Outcome::NeedsModel;
    }
    let transcribing = meetings::set_state(&env.db.conn(), id, "transcribing", None);
    let result = transcribing.and_then(|()| {
        started();
        transcribe(env, id, engines)
    });
    let (state, error, outcome) = match result {
        Ok(item_id) => return Outcome::Done { item_id },
        Err(AppError::Code("cancelled")) => ("queued", None, Outcome::Cancelled),
        // Deleted mid-job: nothing left to update.
        Err(AppError::Code("notFound"))
            if matches!(meetings::get(&env.db.conn(), id), Err(AppError::Code("notFound"))) =>
        {
            return Outcome::Cancelled
        }
        Err(e) => {
            eprintln!("[transcribe] {id} failed: {e}");
            let code = match e {
                AppError::Code(c) if JOB_ERRORS.contains(&c) => c,
                _ => "transcribeFailed",
            };
            ("failed", Some(code), Outcome::Failed(code))
        }
    };
    if let Err(e) = meetings::set_state(&env.db.conn(), id, state, error) {
        eprintln!("[transcribe] {id}: setting {state}: {e}");
    }
    outcome
}

/// Every `*.wav` in `dir` (none when it doesn't exist).
fn wavs(dir: &Path) -> AppResult<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut out = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_some_and(|x| x == "wav") {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// A track's samples; a missing file is an empty track.
fn read_track(path: &Path) -> AppResult<Vec<f32>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    audio::read_wav_f32(path)
}

fn write_wav(path: &Path, samples: &[f32]) -> AppResult<()> {
    let mut w = TrackWriter::create(path, RATE)?;
    w.push(samples, Duration::ZERO)?;
    w.finish()?;
    Ok(())
}

/// Removes a folder; a missing one is fine.
fn remove_dir(dir: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Transcribes one track, reporting progress from `base` over `span`. An empty track is skipped.
fn run_stt(
    stt: &mut dyn SpeechToText,
    samples: &[f32],
    language: &str,
    queue: &TranscribeQueue,
    base: f64,
    span: f64,
) -> AppResult<(Vec<RawSeg>, Option<String>)> {
    if samples.is_empty() {
        return Ok((Vec::new(), None));
    }
    // Called from whisper.cpp through `extern "C"`: must never panic.
    let mut progress = |p: f32| queue.set_progress(base + span * f64::from(p));
    let result = stt.transcribe(samples, language, &mut progress, &queue.cancel)?;
    queue.set_progress(base + span);
    Ok(result)
}

/// Steps 2–8 of a job; returns the meeting item's id.
fn transcribe(env: &Env, id: &str, engines: &dyn Engines) -> AppResult<String> {
    let m = meetings::get(&env.db.conn(), id)?;
    let dir = env.paths.recording_dir(id);
    let convert = engines.convert();

    // Raw tracks (recorder or import rate) → 16 kHz.
    for raw in wavs(&dir)? {
        let Some(stem) = raw.file_stem().and_then(|s| s.to_str()).and_then(|s| s.strip_suffix("-raw")) else {
            continue;
        };
        convert.wav16k(&raw, &dir.join(format!("{stem}.wav")))?;
        std::fs::remove_file(&raw)?;
    }
    let mic = read_track(&dir.join("mic.wav"))?;
    let system = read_track(&dir.join("system.wav"))?;
    if mic.is_empty() && system.is_empty() {
        return Err(AppError::Code("noAudio"));
    }
    env.queue.cancelled()?;

    // Speech to text: one model load per job, mic then system.
    let (mic_segs, system_segs, detected) = {
        let mut stt = engines.speech(env.paths)?;
        let mic_span = if system.is_empty() { 0.9 } else { 0.45 };
        let (mic_segs, mic_lang) = run_stt(stt.as_mut(), &mic, &m.language, env.queue, 0.0, mic_span)?;
        let base = if mic.is_empty() { 0.0 } else { mic_span };
        let (system_segs, system_lang) = run_stt(stt.as_mut(), &system, &m.language, env.queue, base, 0.9 - base)?;
        (mic_segs, system_segs, mic_lang.or(system_lang))
    };

    // Speakers of the system track; a failure leaves the meeting undiarized.
    env.queue.set_progress(0.9);
    let turns = if !system.is_empty() && models::ready(env.paths, Pack::Speaker) {
        match engines.speaker(env.paths).and_then(|d| d.diarize(&system)) {
            Ok(turns) => Some(turns),
            Err(e) => {
                eprintln!("[transcribe] {id}: diarization failed: {e}");
                None
            }
        }
    } else {
        None
    };
    env.queue.set_progress(0.98);
    env.queue.cancelled()?;

    let fallback = match m.language.as_str() {
        "auto" => detected.as_deref().unwrap_or("en"),
        "vi+en" => "vi",
        chosen => chosen,
    };
    let diarized = turns.as_ref().is_some_and(|t| !t.is_empty());
    let (segs, speakers) = assemble(mic_segs, system_segs, turns.as_deref(), &m.source, fallback);
    meetings::save_transcript(&env.db.conn(), id, &segs, &speakers, &langs_of(&segs), diarized)?;

    archive(convert, &dir, &mic, &system)?;

    let item_id = {
        let mut conn = env.db.conn();
        let m = meetings::get(&conn, id)?;
        let sensitive = item_state(&conn, id)?.is_some_and(|(_, _, sensitive)| sensitive);
        write_item(&mut conn, &m, env.lang, None, sensitive)?
    };

    let mut conn = env.db.conn();
    let tx = conn.transaction()?;
    meetings::set_state(&tx, id, "done", None)?;
    if segs.is_empty() {
        meetings::set_summary_state(&tx, id, "skipped", Some("noSpeech"))?;
    } else {
        meetings::set_summary_state(&tx, id, "pending", None)?;
    }
    tx.commit()?;
    if !segs.is_empty() {
        env.queue.poke();
    }
    Ok(item_id)
}

/// Keeps the audio as AAC: `mic.m4a` / `system.m4a` for each track with audio and `mix.m4a`
/// for playback; then deletes every `.wav`.
fn archive(convert: &dyn Convert, dir: &Path, mic: &[f32], system: &[f32]) -> AppResult<()> {
    let (mic_wav, system_wav) = (dir.join("mic.wav"), dir.join("system.wav"));
    if !mic.is_empty() {
        convert.aac(&mic_wav, &dir.join("mic.m4a"))?;
    }
    if !system.is_empty() {
        convert.aac(&system_wav, &dir.join("system.m4a"))?;
    }
    let mix = match (mic.is_empty(), system.is_empty()) {
        (false, false) => {
            let path = dir.join("mix.wav");
            write_wav(&path, &audio::mix(mic, system))?;
            path
        }
        (false, true) => mic_wav,
        _ => system_wav,
    };
    convert.aac(&mix, &dir.join("mix.m4a"))?;
    for wav in wavs(dir)? {
        std::fs::remove_file(wav)?;
    }
    Ok(())
}

/// The meeting item's id, summary and `sensitive` flag, when it has one.
pub(super) fn item_state(conn: &Connection, meeting_id: &str) -> AppResult<Option<(String, Option<String>, bool)>> {
    let Some(item_id) = meetings::item_id(conn, meeting_id)? else { return Ok(None) };
    Ok(conn
        .query_row("SELECT summary, sensitive FROM items WHERE id = ?1", [&item_id], |r| {
            Ok((item_id.clone(), r.get(0)?, r.get(1)?))
        })
        .optional()?)
}

/// Writes the meeting's Inbox item from the row (Markdown without the transcript).
pub(super) fn write_item(
    conn: &mut Connection,
    m: &MeetingRow,
    lang: Lang,
    summary: Option<String>,
    sensitive: bool,
) -> AppResult<String> {
    items::upsert_meeting_item(
        conn,
        &MeetingItem {
            meeting_id: m.id.clone(),
            title: m.title.clone(),
            summary,
            payload: meetings::markdown(m, lang, false),
            sensitive,
            captured_at: m.started_at,
        },
    )
}

/// Rewrites an existing meeting item after a rename (summary and `sensitive` kept). Returns
/// its id; `None` when the meeting has no item yet.
fn refresh_item(conn: &mut Connection, id: &str, lang: Lang) -> AppResult<Option<String>> {
    let Some((_, summary, sensitive)) = item_state(conn, id)? else { return Ok(None) };
    let m = meetings::get(conn, id)?;
    write_item(conn, &m, lang, summary, sensitive).map(Some)
}

// ---------- worker loop ----------

enum Job {
    Transcribe(String),
    Summarize(String),
}

/// The next job: a transcription, else a summary. `None` while held, during a models move or
/// with nothing to do. Without the speech pack queued meetings wait (and show `needsSpeechModel`).
fn next_job(app: &AppHandle) -> AppResult<Option<Job>> {
    let queue = app.state::<TranscribeQueue>();
    if queue.held.load(SeqCst) || queue.models_moving.load(SeqCst) {
        return Ok(None);
    }
    let ready = models::ready(&app.state::<Paths>(), Pack::Speech);
    let (marked, summary) = {
        let db = app.state::<Db>();
        let conn = db.conn();
        let device = export::device_id(&conn)?;
        let queued = meetings::queued_for(&conn, &device)?;
        let mut marked = Vec::new();
        if ready {
            if let Some(id) = queued.into_iter().next() {
                return Ok(Some(Job::Transcribe(id)));
            }
        } else {
            for id in queued {
                if mark_needs_model(&conn, &id)? {
                    marked.push(id);
                }
            }
        }
        (marked, meetings::pending_summaries(&conn, &device)?.into_iter().next())
    };
    if !marked.is_empty() {
        meetings::notify_changed(app, marked);
    }
    Ok(summary.map(Job::Summarize))
}

async fn run(app: AppHandle) {
    let queue = app.state::<TranscribeQueue>();
    loop {
        let failed = match next_job(&app) {
            Ok(Some(Job::Transcribe(id))) => {
                process(&app, &id).await;
                continue;
            }
            Ok(Some(Job::Summarize(id))) => match summary::run_pending(&app, &id).await {
                Ok(()) => continue,
                Err(e) => format!("summary {id}: {e}"),
            },
            Ok(None) => {
                queue.notify.notified().await;
                continue;
            }
            Err(e) => format!("queue: {e}"),
        };
        eprintln!("[transcribe] {failed}");
        tokio::select! {
            _ = tokio::time::sleep(RETRY_AFTER) => {}
            _ = queue.notify.notified() => {}
        }
    }
}

/// While a job runs: `meetings-changed` when progress moved at least 1 %, at most once a second.
fn spawn_progress(app: AppHandle, id: String) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(PROGRESS_TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last = 0.0;
        loop {
            tick.tick().await;
            let progress = app.state::<TranscribeQueue>().progress();
            let Some((_, p)) = progress.filter(|(running, _)| *running == id) else { continue };
            if p - last >= PROGRESS_STEP {
                last = p;
                events::meetings_changed(&app, vec![id.clone()]);
            }
        }
    })
}

/// Runs one transcription job on the blocking pool with the real engines.
async fn process(app: &AppHandle, id: &str) {
    let queue = app.state::<TranscribeQueue>();
    if !queue.begin(id) {
        return;
    }
    let ticker = spawn_progress(app.clone(), id.to_string());
    let lang = i18n::lang(app);
    let (handle, job) = (app.clone(), id.to_string());
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let env = Env {
            db: handle.state::<Db>().inner(),
            paths: handle.state::<Paths>().inner(),
            queue: handle.state::<TranscribeQueue>().inner(),
            lang,
        };
        run_job(&env, &job, &Real, &|| meetings::notify_changed(&handle, vec![job.clone()]))
    })
    .await;
    ticker.abort();
    queue.end();
    let outcome = joined.unwrap_or_else(|e| {
        eprintln!("[transcribe] {id}: job panicked: {e}");
        if let Err(e) = meetings::set_state(&app.state::<Db>().conn(), id, "failed", Some("transcribeFailed")) {
            eprintln!("[transcribe] {id}: {e}");
        }
        Outcome::Failed("transcribeFailed")
    });
    meetings::notify_changed(app, vec![id.to_string()]);
    match outcome {
        Outcome::Done { item_id } => items::notify_changed(app, vec![item_id]),
        Outcome::Failed(_) => events::pet_reaction(app, PetReaction::new("confused")),
        Outcome::NeedsModel | Outcome::Cancelled => {}
    }
}

// ---------- startup ----------

/// The sample rate in a WAV header.
fn wav_rate(path: &Path) -> AppResult<u32> {
    hound::WavReader::open(path)
        .map(|r| r.spec().sample_rate)
        .map_err(|e| AppError::msg(format!("wav {}: {e}", path.display())))
}

/// Crash recovery for this device: interrupted recordings get their WAV headers repaired and
/// are queued when at least 2 s long (else deleted); interrupted transcriptions go back to
/// `queued`, running summaries to `pending`. Returns the meetings changed.
pub(crate) fn recover(db: &Db, paths: &Paths) -> AppResult<Vec<String>> {
    let conn = db.conn();
    let device = export::device_id(&conn)?;
    let mut changed = Vec::new();
    for id in meetings::in_state_for(&conn, &device, "recording")? {
        let dir = paths.recording_dir(&id);
        let (mut duration_ms, mut has_mic, mut has_system) = (0i64, false, false);
        for wav in wavs(&dir)? {
            let samples = match audio::fix_wav_header(&wav) {
                Ok(0) => continue,
                Ok(n) => n,
                Err(e) => {
                    eprintln!("[transcribe] recovering {}: {e}", wav.display());
                    continue;
                }
            };
            let rate = match wav_rate(&wav) {
                Ok(rate) if rate > 0 => rate,
                Ok(_) => continue,
                Err(e) => {
                    eprintln!("[transcribe] recovering: {e}");
                    continue;
                }
            };
            duration_ms = duration_ms.max((samples * 1000 / u64::from(rate)) as i64);
            if wav.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("mic")) {
                has_mic = true;
            } else {
                has_system = true;
            }
        }
        if duration_ms >= MIN_RECOVERED_MS {
            meetings::set_duration(&conn, &id, duration_ms, has_mic, has_system)?;
            meetings::set_state(&conn, &id, "queued", None)?;
        } else {
            if let Err(e) = remove_dir(&dir) {
                eprintln!("[transcribe] deleting {}: {e}", dir.display());
            }
            meetings::soft_delete(&conn, &id)?;
        }
        changed.push(id);
    }
    for id in meetings::in_state_for(&conn, &device, "transcribing")? {
        meetings::set_state(&conn, &id, "queued", None)?;
        changed.push(id);
    }
    for id in meetings::summary_in_state_for(&conn, &device, Some("running"), None)? {
        meetings::set_summary_state(&conn, &id, "pending", None)?;
        changed.push(id);
    }
    Ok(changed)
}

/// When `transcribe.audioRetentionDays` is a number `n`: deletes the audio folders of this
/// device's finished meetings that started more than `n` days before `now`. Returns them.
pub(crate) fn sweep_retention(db: &Db, paths: &Paths, now: i64) -> AppResult<Vec<String>> {
    let conn = db.conn();
    let Some(days) = settings::get_value(&conn, RETENTION_SETTING)?.and_then(|v| v.as_i64()) else {
        return Ok(Vec::new());
    };
    let device = export::device_id(&conn)?;
    let mut swept = Vec::new();
    for id in meetings::audio_expired(&conn, &device, now - days.saturating_mul(DAY_MS))? {
        let dir = paths.recording_dir(&id);
        if let Err(e) = remove_dir(&dir) {
            eprintln!("[transcribe] retention: deleting {}: {e}", dir.display());
            continue;
        }
        meetings::set_audio_deleted(&conn, &id, now)?;
        swept.push(id);
    }
    Ok(swept)
}

/// The retention sweep at start, every 6 h and whenever its setting changes.
fn start_retention(app: &AppHandle) {
    let wake = Arc::new(Notify::new());
    let on_change = wake.clone();
    app.listen_any(events::SETTINGS_CHANGED, move |event| {
        let key = serde_json::from_str::<Value>(event.payload()).ok();
        if key.as_ref().and_then(|v| v["key"].as_str()) == Some(RETENTION_SETTING) {
            on_change.notify_one();
        }
    });
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let handle = app.clone();
            let swept = tauri::async_runtime::spawn_blocking(move || {
                sweep_retention(handle.state::<Db>().inner(), handle.state::<Paths>().inner(), now_ms())
            })
            .await;
            match swept {
                Ok(Ok(ids)) if !ids.is_empty() => meetings::notify_changed(&app, ids),
                Ok(Ok(_)) => {}
                Ok(Err(e)) => eprintln!("[transcribe] retention: {e}"),
                Err(e) => eprintln!("[transcribe] retention: {e}"),
            }
            tokio::select! {
                _ = tokio::time::sleep(RETENTION_EVERY) => {}
                _ = wake.notified() => {}
            }
        }
    });
}

/// Recovers interrupted work, then spawns the worker and the retention loop. Needs `Db`,
/// `Paths` and `TranscribeQueue` managed.
pub fn start(app: &AppHandle) {
    match recover(app.state::<Db>().inner(), app.state::<Paths>().inner()) {
        Ok(ids) if !ids.is_empty() => meetings::notify_changed(app, ids),
        Ok(_) => {}
        Err(e) => eprintln!("[transcribe] recovery: {e}"),
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move { run(handle).await });
    start_retention(app);
}

/// A model or provider became usable: summaries skipped for `noModel`, `notSetUp` or
/// `cloudMeetingsOff` are pending again.
pub fn summaries_unblocked(app: &AppHandle) {
    let result = (|| -> AppResult<Vec<String>> {
        let conn = app.state::<Db>().inner().conn();
        let device = export::device_id(&conn)?;
        let mut ids = Vec::new();
        for code in ["noModel", "notSetUp", "cloudMeetingsOff"] {
            ids.extend(meetings::summary_in_state_for(&conn, &device, None, Some(code))?);
        }
        for id in &ids {
            meetings::set_summary_state(&conn, id, "pending", None)?;
        }
        Ok(ids)
    })();
    match result {
        Ok(ids) if !ids.is_empty() => {
            meetings::notify_changed(app, ids);
            poke(app);
        }
        Ok(_) => {}
        Err(e) => eprintln!("[transcribe] requeue summaries: {e}"),
    }
}

// ---------- imports ----------

/// Imports an audio or video file as a meeting (`source = import`, the `transcribe.language`
/// setting) and queues it. Errors: `unsupportedAudio`.
pub async fn import(
    app: &AppHandle,
    path: &Path,
    title: &str,
    source_item_id: Option<&str>,
    started_at: i64,
) -> AppResult<String> {
    let tmp = app.state::<Paths>().tmp.join(format!("import-{}.wav", crate::db::new_id()));
    let (input, out) = (path.to_path_buf(), tmp.clone());
    let duration_ms = tauri::async_runtime::spawn_blocking(move || audio::decode_to_wav(&input, &out))
        .await
        .map_err(|e| AppError::msg(e.to_string()))??;
    let id = place_import(
        app.state::<Db>().inner(),
        app.state::<Paths>().inner(),
        &tmp,
        title,
        source_item_id,
        started_at,
        duration_ms as i64,
    )?;
    meetings::notify_changed(app, vec![id.clone()]);
    poke(app);
    Ok(id)
}

/// Creates the import's meeting and moves the decoded track `tmp` into its folder as
/// `system-raw.wav`, in one transaction under the connection lock: the worker can't see the
/// queued row before its audio is in place, and on any error neither the row nor the folder
/// (nor `tmp`) is left.
fn place_import(
    db: &Db,
    paths: &Paths,
    tmp: &Path,
    title: &str,
    source_item_id: Option<&str>,
    started_at: i64,
    duration_ms: i64,
) -> AppResult<String> {
    let conn = db.conn();
    let mut dir = None;
    let placed = (|| -> AppResult<String> {
        let tx = conn.unchecked_transaction()?;
        let language: String = settings::get(&tx, "transcribe.language")?;
        let device = export::device_id(&tx)?;
        let id = meetings::create(&tx, "import", &language, &device, started_at, title, source_item_id)?;
        let d = dir.insert(paths.recording_dir(&id));
        std::fs::create_dir_all(&*d)?;
        std::fs::rename(tmp, d.join("system-raw.wav"))?;
        meetings::set_duration(&tx, &id, duration_ms, false, true)?;
        tx.commit()?;
        Ok(id)
    })();
    if placed.is_err() {
        let _ = std::fs::remove_file(tmp);
        if let Some(dir) = dir {
            if let Err(e) = remove_dir(&dir) {
                eprintln!("[transcribe] deleting {}: {e}", dir.display());
            }
        }
    }
    placed
}

/// `import` of a file the user picked: titled after the file, started at its modification time.
pub async fn import_file(app: &AppHandle, path: &Path) -> AppResult<String> {
    let title = path.file_stem().unwrap_or(path.as_os_str()).to_string_lossy().into_owned();
    let started_at = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or_else(now_ms, |d| d.as_millis() as i64);
    import(app, path, &title, None, started_at).await
}

/// `import` of a live audio or video item's file. Errors: `invalid` for anything else.
pub async fn transcribe_item(app: &AppHandle, item_id: &str) -> AppResult<String> {
    let (path, title, started_at) = {
        let row = items::get_row(&app.state::<Db>().conn(), item_id)?;
        let rel = match (&row.file_path, &row.mime) {
            (Some(rel), Some(mime)) if row.deleted_at.is_none() && is_media(mime) => rel.clone(),
            _ => return Err(AppError::Code("invalid")),
        };
        let stem = row.file_name.as_deref().and_then(|n| Path::new(n).file_stem()).map(|s| s.to_string_lossy());
        let title = match stem {
            Some(s) if !s.trim().is_empty() => s.into_owned(),
            _ => row.title.clone(),
        };
        (app.state::<Paths>().abs(&rel), title, row.captured_at)
    };
    import(app, &path, &title, Some(item_id), started_at).await
}

// ---------- meeting commands ----------

fn progress_ref(p: &Option<(String, f64)>) -> Option<(&str, f64)> {
    p.as_ref().map(|(id, v)| (id.as_str(), *v))
}

pub fn list(app: &AppHandle) -> AppResult<Vec<MeetingSummary>> {
    let progress = app.state::<TranscribeQueue>().progress();
    let conn = app.state::<Db>().inner().conn();
    meetings::summaries(&conn, &export::device_id(&conn)?, &app.state::<Paths>(), progress_ref(&progress))
}

pub fn detail(app: &AppHandle, id: &str) -> AppResult<MeetingDetail> {
    let progress = app.state::<TranscribeQueue>().progress();
    let conn = app.state::<Db>().inner().conn();
    meetings::detail(&conn, id, &export::device_id(&conn)?, &app.state::<Paths>(), progress_ref(&progress))
}

/// Emits `meetings-changed` for `id` and `items-changed` for its item.
fn notify(app: &AppHandle, id: &str, item: Option<String>) {
    meetings::notify_changed(app, vec![id.to_string()]);
    if let Some(item) = item {
        items::notify_changed(app, vec![item]);
    }
}

/// Renames the meeting and its item.
pub fn rename(app: &AppHandle, id: &str, title: &str) -> AppResult<()> {
    let lang = i18n::lang(app);
    let item = {
        let mut conn = app.state::<Db>().inner().conn();
        meetings::rename(&conn, id, title)?;
        refresh_item(&mut conn, id, lang)?
    };
    notify(app, id, item);
    Ok(())
}

/// Names a speaker (`None` restores the default label) and refreshes the meeting item.
pub fn rename_speaker(app: &AppHandle, id: &str, key: &str, name: Option<&str>) -> AppResult<()> {
    let lang = i18n::lang(app);
    let item = {
        let mut conn = app.state::<Db>().inner().conn();
        meetings::rename_speaker(&conn, id, key, name)?;
        refresh_item(&mut conn, id, lang)?
    };
    notify(app, id, item);
    Ok(())
}

/// Queues a failed meeting again. Other states are `invalid`; without its audio `audioGone`.
pub fn retry(app: &AppHandle, id: &str) -> AppResult<()> {
    {
        let conn = app.state::<Db>().inner().conn();
        let m = meetings::get(&conn, id)?;
        if m.state != "failed" {
            return Err(AppError::Code("invalid"));
        }
        if m.audio_deleted_at.is_some() {
            return Err(AppError::Code("audioGone"));
        }
        meetings::set_state(&conn, id, "queued", None)?;
    }
    notify(app, id, None);
    poke(app);
    Ok(())
}

/// Transcribes a finished meeting again in `language` from its archived audio. Errors:
/// `notOnThisMac`, `audioGone`, `busy`, `invalid`, `audioConvert`.
pub async fn retranscribe(app: &AppHandle, id: &str, language: &str) -> AppResult<()> {
    if !meetings::LANGUAGES.contains(&language) {
        return Err(AppError::Code("invalid"));
    }
    let dir = {
        let conn = app.state::<Db>().inner().conn();
        let m = meetings::get(&conn, id)?;
        if m.device_id != export::device_id(&conn)? {
            return Err(AppError::Code("notOnThisMac"));
        }
        let dir = app.state::<Paths>().recording_dir(id);
        if m.audio_deleted_at.is_some() || !dir.join("mix.m4a").exists() {
            return Err(AppError::Code("audioGone"));
        }
        if matches!(m.state.as_str(), "transcribing" | "queued") {
            return Err(AppError::Code("busy"));
        }
        dir
    };
    tauri::async_runtime::spawn_blocking(move || -> AppResult<()> {
        let converted = ["mic", "system"].iter().try_for_each(|track| -> AppResult<()> {
            let m4a = dir.join(format!("{track}.m4a"));
            if m4a.exists() {
                NativeConvert.wav16k(&m4a, &dir.join(format!("{track}-raw.wav")))?;
            }
            Ok(())
        });
        if converted.is_err() {
            for track in ["mic", "system"] {
                let _ = std::fs::remove_file(dir.join(format!("{track}-raw.wav")));
            }
        }
        converted
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))??;
    {
        let conn = app.state::<Db>().inner().conn();
        meetings::set_language(&conn, id, language)?;
        meetings::set_state(&conn, id, "queued", None)?;
    }
    notify(app, id, None);
    poke(app);
    Ok(())
}

/// Deletes a meeting: cancels its transcription, removes its audio, soft-deletes the row and
/// trashes its item. `busy` while it is being recorded.
pub fn delete(app: &AppHandle, id: &str) -> AppResult<()> {
    if app.state::<Recorder>().status().meeting_id.as_deref() == Some(id) {
        return Err(AppError::Code("busy"));
    }
    app.state::<TranscribeQueue>().cancel_if(id);
    let item = {
        let mut conn = app.state::<Db>().inner().conn();
        meetings::soft_delete(&conn, id)?;
        let item = meetings::item_id(&conn, id)?;
        if let Some(item) = &item {
            items::trash(&mut conn, std::slice::from_ref(item), &Actor::User)?;
        }
        item
    };
    let dir = app.state::<Paths>().recording_dir(id);
    if let Err(e) = remove_dir(&dir) {
        eprintln!("[transcribe] deleting {}: {e}", dir.display());
    }
    notify(app, id, item);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::db::items::get_row;
    use crate::transcribe::diarize::Turn;

    /// `NativeConvert` stand-in: copies the file (test tracks are already 16 kHz WAVs).
    struct CopyConvert;

    impl Convert for CopyConvert {
        fn wav16k(&self, i: &Path, o: &Path) -> AppResult<()> {
            std::fs::copy(i, o)?;
            Ok(())
        }

        fn aac(&self, i: &Path, o: &Path) -> AppResult<()> {
            std::fs::copy(i, o)?;
            Ok(())
        }
    }

    type Segs = Vec<(i64, i64, &'static str)>;

    /// Returns the planned segments per call (mic first); `cancel` makes every call cancel the
    /// running job the way `pause_for_recording` does.
    struct FakeStt {
        calls: VecDeque<Segs>,
        cancel: Option<Arc<TranscribeQueue>>,
    }

    impl SpeechToText for FakeStt {
        fn transcribe(
            &mut self,
            _samples: &[f32],
            _language: &str,
            progress: &mut dyn FnMut(f32),
            cancel: &AtomicBool,
        ) -> AppResult<(Vec<RawSeg>, Option<String>)> {
            progress(0.5);
            if let Some(q) = &self.cancel {
                q.pause_for_recording();
            }
            if cancel.load(SeqCst) {
                return Err(AppError::Code("cancelled"));
            }
            let segs = self.calls.pop_front().unwrap_or_default();
            let lang = (!segs.is_empty()).then(|| "en".to_string());
            let segs = segs.into_iter().map(|(s, e, t)| RawSeg { start_ms: s, end_ms: e, text: t.into() }).collect();
            Ok((segs, lang))
        }
    }

    struct FakeDiarizer(Vec<(i64, i64, i32)>);

    impl Diarizer for FakeDiarizer {
        fn diarize(&self, _samples: &[f32]) -> AppResult<Vec<Turn>> {
            Ok(self.0.iter().map(|&(start_ms, end_ms, speaker)| Turn { start_ms, end_ms, speaker }).collect())
        }
    }

    #[derive(Default)]
    struct Fakes {
        calls: Vec<Segs>,
        turns: Vec<(i64, i64, i32)>,
        cancel: Option<Arc<TranscribeQueue>>,
        loads: Mutex<usize>,
    }

    impl Engines for Fakes {
        fn convert(&self) -> &dyn Convert {
            &CopyConvert
        }

        fn speech(&self, _paths: &Paths) -> AppResult<Box<dyn SpeechToText>> {
            *lock(&self.loads) += 1;
            Ok(Box::new(FakeStt { calls: self.calls.clone().into(), cancel: self.cancel.clone() }))
        }

        fn speaker(&self, _paths: &Paths) -> AppResult<Box<dyn Diarizer>> {
            Ok(Box::new(FakeDiarizer(self.turns.clone())))
        }
    }

    struct Setup {
        _tmp: tempfile::TempDir,
        db: Db,
        paths: Paths,
        device: String,
    }

    fn setup() -> Setup {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::new(tmp.path().to_path_buf());
        paths.ensure().unwrap();
        let db = Db::open_in_memory().unwrap();
        let device = export::device_id(&db.conn()).unwrap();
        Setup { _tmp: tmp, db, paths, device }
    }

    fn install(paths: &Paths, pack: &[crate::llm::models::ModelSpec]) {
        for spec in pack {
            std::fs::write(model_path(paths, spec), b"model").unwrap();
        }
    }

    fn tone(secs: u32, rate: u32) -> Vec<f32> {
        (0..secs * rate).map(|i| ((i % 100) as f32 / 100.0) - 0.5).collect()
    }

    fn wav(path: &Path, rate: u32, samples: &[f32]) {
        let mut w = TrackWriter::create(path, rate).unwrap();
        w.push(samples, Duration::ZERO).unwrap();
        w.finish().unwrap();
    }

    /// A recording meeting with the given raw tracks in its folder, then queued.
    fn queued(s: &Setup, source: &str, tracks: &[&str]) -> String {
        let conn = s.db.conn();
        let id = meetings::create(&conn, source, "en", &s.device, 1_000, "Sync", None).unwrap();
        meetings::set_state(&conn, &id, "queued", None).unwrap();
        let dir = s.paths.recording_dir(&id);
        std::fs::create_dir_all(&dir).unwrap();
        for t in tracks {
            wav(&dir.join(t), RATE, &tone(1, RATE));
        }
        id
    }

    fn env<'a>(s: &'a Setup, queue: &'a TranscribeQueue) -> Env<'a> {
        Env { db: &s.db, paths: &s.paths, queue, lang: Lang::En }
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    #[test]
    fn missing_speech_pack_keeps_queued() {
        let s = setup();
        let id = queued(&s, "recording", &["mic-raw.wav"]);
        let queue = TranscribeQueue::default();
        let fakes = Fakes::default();
        let outcome = run_job(&env(&s, &queue), &id, &fakes, &|| panic!("must not start"));
        assert_eq!(outcome, Outcome::NeedsModel);
        let m = meetings::get(&s.db.conn(), &id).unwrap();
        assert_eq!((m.state.as_str(), m.error.as_deref()), ("queued", Some("needsSpeechModel")));
        assert_eq!(*lock(&fakes.loads), 0, "no model load without the pack");
        assert_eq!(files(&s.paths.recording_dir(&id)), ["mic-raw.wav"], "audio untouched");
    }

    #[test]
    fn cancel_requeues_without_failing() {
        let s = setup();
        install(&s.paths, &SPEECH);
        let id = queued(&s, "recording", &["mic-raw.wav"]);
        let queue = Arc::new(TranscribeQueue::default());
        assert!(queue.begin(&id));
        let fakes = Fakes { cancel: Some(queue.clone()), ..Default::default() };
        let outcome = run_job(&env(&s, &queue), &id, &fakes, &|| {});
        assert_eq!(outcome, Outcome::Cancelled);
        let m = meetings::get(&s.db.conn(), &id).unwrap();
        assert_eq!((m.state.as_str(), m.error), ("queued", None));
        assert!(m.transcript.is_empty());
        assert_eq!(files(&s.paths.recording_dir(&id)), ["mic.wav"], "converted audio kept for the rerun");
        assert!(!queue.begin(&id), "held until the recording ends");
        queue.resume_after_recording();
        assert!(queue.begin(&id));
    }

    #[test]
    fn models_move_waits_for_the_running_job() {
        let queue = TranscribeQueue::default();
        assert!(queue.begin("m"));
        assert!(!queue.hold_for_models_move(), "a transcription is running");
        queue.end();
        assert!(queue.hold_for_models_move());
        assert!(!queue.begin("m"), "held while the models move");
        queue.end_models_move();
        assert!(queue.begin("m"));
    }

    #[test]
    fn meeting_deleted_mid_job_is_not_a_failure() {
        let s = setup();
        install(&s.paths, &SPEECH);
        let id = queued(&s, "recording", &["mic-raw.wav"]);
        let queue = TranscribeQueue::default();
        let outcome = run_job(&env(&s, &queue), &id, &Fakes::default(), &|| {
            meetings::soft_delete(&s.db.conn(), &id).unwrap();
        });
        assert_eq!(outcome, Outcome::Cancelled);
    }

    #[test]
    fn no_speech_skips_summary() {
        let s = setup();
        install(&s.paths, &SPEECH);
        let id = queued(&s, "recording", &["mic-raw.wav"]);
        let queue = TranscribeQueue::default();
        let outcome = run_job(&env(&s, &queue), &id, &Fakes::default(), &|| {});
        let Outcome::Done { item_id } = outcome else { panic!("{outcome:?}") };
        let m = meetings::get(&s.db.conn(), &id).unwrap();
        assert_eq!(m.state, "done");
        assert!(m.transcript.is_empty());
        assert_eq!((m.summary_state.as_str(), m.summary_error.as_deref()), ("skipped", Some("noSpeech")));
        assert_eq!(get_row(&s.db.conn(), &item_id).unwrap().meeting_id.as_deref(), Some(id.as_str()));
        assert_eq!(files(&s.paths.recording_dir(&id)), ["mic.m4a", "mix.m4a"]);
    }

    #[test]
    fn transcript_and_meeting_item_saved() {
        let s = setup();
        install(&s.paths, &SPEECH);
        install(&s.paths, &SPEAKER);
        let id = queued(&s, "recording", &["mic-raw.wav", "system.wav"]);
        let queue = TranscribeQueue::default();
        let fakes = Fakes {
            calls: vec![
                vec![(0, 400, "Hello team, let's start")],
                vec![(300, 600, "The API contract has two open comments"), (700, 900, "I can benchmark it next week")],
            ],
            turns: vec![(300, 600, 4), (700, 900, 1)],
            ..Default::default()
        };
        let started = Mutex::new(false);
        let outcome = run_job(&env(&s, &queue), &id, &fakes, &|| *lock(&started) = true);
        let Outcome::Done { item_id } = outcome else { panic!("{outcome:?}") };
        assert!(*lock(&started));
        assert_eq!(*lock(&fakes.loads), 1, "one model load for both tracks");

        let conn = s.db.conn();
        let m = meetings::get(&conn, &id).unwrap();
        assert_eq!((m.state.as_str(), m.error.as_deref()), ("done", None));
        let speakers: Vec<&str> = m.transcript.iter().map(|t| t.speaker.as_str()).collect();
        assert_eq!(speakers, ["me", "s1", "s2"]);
        assert_eq!(m.transcript[1].text, "The API contract has two open comments");
        assert!(m.diarized);
        assert_eq!(m.langs, ["en"]);
        assert_eq!((m.summary_state.as_str(), m.summary_error.as_deref()), ("pending", None));

        let item = get_row(&conn, &item_id).unwrap();
        assert_eq!(item.title, "Sync");
        assert_eq!(item.summary, None);
        assert_eq!(item.payload_text.as_deref(), Some(meetings::markdown(&m, Lang::En, false).as_str()));
        assert_eq!(meetings::item_id(&conn, &id).unwrap().as_deref(), Some(item_id.as_str()));
        assert_eq!(files(&s.paths.recording_dir(&id)), ["mic.m4a", "mix.m4a", "system.m4a"]);
    }

    #[test]
    fn retention_deletes_only_old_local_audio() {
        let s = setup();
        let now = 100 * DAY_MS;
        settings::put(&s.db.conn(), RETENTION_SETTING, &serde_json::json!(7)).unwrap();
        let meeting = |device: &str, days_ago: i64, state: &str| {
            let conn = s.db.conn();
            let id = meetings::create(&conn, "recording", "en", device, now - days_ago * DAY_MS, "M", None).unwrap();
            meetings::set_state(&conn, &id, state, None).unwrap();
            let dir = s.paths.recording_dir(&id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("mix.m4a"), b"aac").unwrap();
            id
        };
        let old_done = meeting(&s.device, 8, "done");
        let old_failed = meeting(&s.device, 30, "failed");
        let recent = meeting(&s.device, 6, "done");
        let old_queued = meeting(&s.device, 8, "queued");
        let elsewhere = meeting("other-mac", 8, "done");

        let swept = sweep_retention(&s.db, &s.paths, now).unwrap();
        assert_eq!(swept.len(), 2);
        assert!(swept.contains(&old_done) && swept.contains(&old_failed));
        for id in [&old_done, &old_failed] {
            assert!(!s.paths.recording_dir(id).exists());
            assert_eq!(meetings::get(&s.db.conn(), id).unwrap().audio_deleted_at, Some(now));
        }
        for id in [&recent, &old_queued, &elsewhere] {
            assert!(s.paths.recording_dir(id).join("mix.m4a").exists());
            assert_eq!(meetings::get(&s.db.conn(), id).unwrap().audio_deleted_at, None);
        }
        assert!(sweep_retention(&s.db, &s.paths, now).unwrap().is_empty(), "already swept");

        settings::put(&s.db.conn(), RETENTION_SETTING, &Value::Null).unwrap();
        assert!(sweep_retention(&s.db, &s.paths, now + 365 * DAY_MS).unwrap().is_empty(), "keep forever");
    }

    /// Zeroes the RIFF and `data` sizes, as a crash before the first header rewrite leaves them.
    fn crash_header(path: &Path) {
        use std::io::{Seek, SeekFrom, Write};
        let mut f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        for at in [4, 40] {
            f.seek(SeekFrom::Start(at)).unwrap();
            f.write_all(&0u32.to_le_bytes()).unwrap();
        }
    }

    #[test]
    fn recovery_requeues_interrupted_recording() {
        let s = setup();
        let recording = |tracks: &[(&str, u32, u32)]| {
            let id = meetings::create(&s.db.conn(), "recording", "en", &s.device, 1_000, "M", None).unwrap();
            let dir = s.paths.recording_dir(&id);
            std::fs::create_dir_all(&dir).unwrap();
            for &(name, rate, secs) in tracks {
                let path = dir.join(name);
                wav(&path, rate, &tone(secs, rate));
                crash_header(&path);
            }
            id
        };
        let long = recording(&[("mic-raw.wav", 48_000, 3), ("system.wav", RATE, 1)]);
        let short = recording(&[("mic-raw.wav", 48_000, 1)]);
        let system_only = recording(&[("system.wav", RATE, 2)]);
        let elsewhere = {
            let conn = s.db.conn();
            meetings::create(&conn, "recording", "en", "other-mac", 1_000, "M", None).unwrap()
        };
        let summarizing = queued(&s, "recording", &[]);
        meetings::set_state(&s.db.conn(), &summarizing, "done", None).unwrap();
        meetings::set_summary_state(&s.db.conn(), &summarizing, "running", None).unwrap();
        let interrupted = queued(&s, "recording", &[]);
        meetings::set_state(&s.db.conn(), &interrupted, "transcribing", None).unwrap();

        let mut changed = recover(&s.db, &s.paths).unwrap();
        changed.sort();
        let mut expected = vec![long.clone(), short.clone(), system_only.clone(), summarizing.clone(), interrupted.clone()];
        expected.sort();
        assert_eq!(changed, expected);

        let conn = s.db.conn();
        let m = meetings::get(&conn, &long).unwrap();
        assert_eq!((m.state.as_str(), m.duration_ms, m.has_mic, m.has_system), ("queued", 3_000, true, true));
        let mic = audio::read_wav_f32(&s.paths.recording_dir(&long).join("mic-raw.wav")).unwrap();
        assert_eq!(mic.len(), 144_000, "header repaired");
        let m = meetings::get(&conn, &system_only).unwrap();
        assert_eq!((m.state.as_str(), m.duration_ms, m.has_mic, m.has_system), ("queued", 2_000, false, true));
        assert!(matches!(meetings::get(&conn, &short), Err(AppError::Code("notFound"))), "under 2 s is deleted");
        assert!(!s.paths.recording_dir(&short).exists());
        assert_eq!(meetings::get(&conn, &elsewhere).unwrap().state, "recording", "another Mac's recording");
        assert_eq!(meetings::get(&conn, &summarizing).unwrap().summary_state, "pending");
        assert_eq!(meetings::get(&conn, &interrupted).unwrap().state, "queued");
    }

    #[test]
    fn import_places_audio_before_queueing() {
        let s = setup();
        let tmp = s.paths.tmp.join("import.wav");
        wav(&tmp, 44_100, &tone(1, 44_100));
        let id = place_import(&s.db, &s.paths, &tmp, "Call", None, 5_000, 1_000).unwrap();
        assert!(!tmp.exists());
        assert_eq!(files(&s.paths.recording_dir(&id)), ["system-raw.wav"]);
        let m = meetings::get(&s.db.conn(), &id).unwrap();
        assert_eq!(
            (m.state.as_str(), m.source.as_str(), m.duration_ms, m.has_mic, m.has_system),
            ("queued", "import", 1_000, false, true)
        );
        assert_eq!(meetings::queued_for(&s.db.conn(), &s.device).unwrap(), [id]);

        // The decoded track is gone: no row (not even a deleted one) and no folder are left.
        let err = place_import(&s.db, &s.paths, &tmp, "Call", None, 5_000, 1_000);
        assert!(matches!(err, Err(AppError::Io(_))), "{err:?}");
        let rows: i64 = s.db.conn().query_row("SELECT COUNT(*) FROM meetings", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 1);
        assert_eq!(std::fs::read_dir(&s.paths.recordings).unwrap().count(), 1);
    }
}
