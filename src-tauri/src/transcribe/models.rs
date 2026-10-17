//! Speech (whisper + VAD) and speaker (diarization) model packs: catalog, sequential
//! resumable download through [`fetch_model`], and `transcribe.download` persistence.
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};
use tauri::{AppHandle, Manager};

use crate::db::{settings, Db};
use crate::error::{AppError, AppResult};
use crate::events;
use crate::llm::download::{fetch_model, Fetched, Progress};
use crate::llm::models::{model_path, part_path, ModelSpec};
use crate::model::{PackStatus, SpeechStatus};
use crate::paths::Paths;

const EMIT_EVERY: Duration = Duration::from_millis(500);
const DOWNLOAD_SETTING: &str = "transcribe.download";

pub const SPEECH: [ModelSpec; 2] = [
    ModelSpec {
        id: "whisper-large-v3-turbo-q5_0",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        file: "ggml-large-v3-turbo-q5_0.bin",
        bytes: 574_041_195,
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        label: "Whisper large-v3-turbo",
    },
    ModelSpec {
        id: "silero-vad-v5.1.2",
        url: "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin",
        file: "ggml-silero-v5.1.2.bin",
        bytes: 885_098,
        sha256: "29940d98d42b91fbd05ce489f3ecf7c72f0a42f027e4875919a28fb4c04ea2cf",
        label: "Silero VAD",
    },
];

pub const SPEAKER: [ModelSpec; 2] = [
    ModelSpec {
        id: "pyannote-segmentation-3-0",
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/main/model.onnx",
        file: "pyannote-segmentation-3-0.onnx",
        bytes: 5_992_913,
        sha256: "220ad67ca923bef2fa91f2390c786097bf305bceb5e261d4af67b38e938e1079",
        label: "pyannote segmentation 3.0",
    },
    ModelSpec {
        id: "3dspeaker-eres2net-base",
        url: "https://huggingface.co/csukuangfj/speaker-embedding-models/resolve/main/3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx",
        file: "3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx",
        bytes: 39_593_761,
        sha256: "1a331345f04805badbb495c775a6ddffcdd1a732567d5ec8b3d5749e3c7a5e4b",
        label: "3D-Speaker ERes2Net",
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pack {
    Speech,
    Speaker,
}

const PACKS: [Pack; 2] = [Pack::Speech, Pack::Speaker];

impl Pack {
    pub fn as_str(self) -> &'static str {
        match self {
            Pack::Speech => "speech",
            Pack::Speaker => "speaker",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

impl FromStr for Pack {
    type Err = AppError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "speech" => Ok(Pack::Speech),
            "speaker" => Ok(Pack::Speaker),
            _ => Err(AppError::Code("invalid")),
        }
    }
}

pub fn files(p: Pack) -> &'static [ModelSpec] {
    match p {
        Pack::Speech => &SPEECH,
        Pack::Speaker => &SPEAKER,
    }
}

fn total_bytes(p: Pack) -> u64 {
    files(p).iter().map(|f| f.bytes).sum()
}

/// Every file of the pack is present (verified) in `models/`.
pub fn ready(paths: &Paths, p: Pack) -> bool {
    files(p).iter().all(|f| model_path(paths, f).exists())
}

/// Bytes on disk: finished files plus their `.part` files.
fn bytes_on_disk(paths: &Paths, p: Pack) -> u64 {
    files(p)
        .iter()
        .map(|f| {
            if model_path(paths, f).exists() {
                f.bytes
            } else {
                std::fs::metadata(part_path(paths, f)).map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}

struct Inner {
    /// `none | downloading | paused | verifying | ready | failed`
    state: &'static str,
    bytes_done: u64,
    error: Option<String>,
    /// Cancel flag of the current transfer task; identifies it as current.
    cancel: Option<Arc<AtomicBool>>,
}

struct Slot {
    inner: Arc<Mutex<Inner>>,
    /// Held by a transfer task for its whole run (one transfer per pack).
    transfer: Arc<tokio::sync::Mutex<()>>,
}

impl Default for Slot {
    fn default() -> Self {
        Slot {
            inner: Arc::new(Mutex::new(Inner { state: "none", bytes_done: 0, error: None, cancel: None })),
            transfer: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}

/// Managed state (`app.state::<PackDownloader>()`).
#[derive(Default)]
pub struct PackDownloader {
    slots: [Slot; 2],
}

fn lock(inner: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    inner.lock().unwrap_or_else(|e| e.into_inner())
}

fn read_persisted(app: &AppHandle) -> Map<String, Value> {
    // `transcribe.download` is not seeded, so it may be absent.
    settings::get_value(&app.state::<Db>().conn(), DOWNLOAD_SETTING)
        .ok()
        .flatten()
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

fn persist(app: &AppHandle, p: Pack, state: &str) {
    let mut m = read_persisted(app);
    m.insert(p.as_str().into(), json!(state));
    if let Err(e) = settings::set(app, DOWNLOAD_SETTING, &Value::Object(m)) {
        eprintln!("[transcribe] persist {DOWNLOAD_SETTING} failed: {e}");
    }
}

fn emit(app: &AppHandle) {
    events::speech_status(app, app.state::<PackDownloader>().status(app));
}

/// Downloads the pack's missing files one after another.
async fn fetch_pack(
    paths: &Paths,
    p: Pack,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(Progress),
) -> AppResult<Fetched> {
    let mut finished = 0u64;
    for spec in files(p) {
        if !model_path(paths, spec).exists() {
            let base = finished;
            let r = fetch_model(paths, spec, cancel, |pr| match pr {
                Progress::Bytes(n) => on_progress(Progress::Bytes(base + n)),
                Progress::Verifying => on_progress(Progress::Verifying),
            })
            .await?;
            if let Fetched::Cancelled = r {
                return Ok(Fetched::Cancelled);
            }
        }
        finished += spec.bytes;
        on_progress(Progress::Bytes(finished));
    }
    Ok(Fetched::Done)
}

impl PackDownloader {
    /// Starts (or resumes) downloading the pack. Errors: `localOnly`.
    pub fn start(&self, app: &AppHandle, p: Pack) -> AppResult<()> {
        if settings::read::<bool>(app, "privacy.localOnly")? {
            return Err(AppError::Code("localOnly"));
        }
        let paths = app.state::<Paths>().inner().clone();
        let slot = &self.slots[p.index()];
        let cancel = Arc::new(AtomicBool::new(false));
        let is_ready = ready(&paths, p);
        {
            let mut inner = lock(&slot.inner);
            if let Some(old) = inner.cancel.take() {
                old.store(true, Ordering::SeqCst);
            }
            inner.error = None;
            if is_ready {
                inner.state = "ready";
                inner.bytes_done = total_bytes(p);
            } else {
                inner.state = "downloading";
                inner.bytes_done = bytes_on_disk(&paths, p);
                inner.cancel = Some(cancel.clone());
            }
        }
        if is_ready {
            persist(app, p, "ready");
            emit(app);
            super::worker::poke(app);
            return Ok(());
        }
        persist(app, p, "downloading");
        emit(app);

        let app = app.clone();
        let inner = slot.inner.clone();
        let transfer = slot.transfer.clone();
        tauri::async_runtime::spawn(async move {
            let _guard = transfer.lock().await;
            let mut last_emit = Instant::now();
            let result = fetch_pack(&paths, p, &cancel, |pr| {
                {
                    let mut i = lock(&inner);
                    if !i.cancel.as_ref().is_some_and(|c| Arc::ptr_eq(c, &cancel)) {
                        return;
                    }
                    match pr {
                        Progress::Bytes(n) => {
                            i.bytes_done = n;
                            i.state = "downloading";
                        }
                        Progress::Verifying => i.state = "verifying",
                    }
                }
                if matches!(pr, Progress::Verifying) || last_emit.elapsed() >= EMIT_EVERY {
                    last_emit = Instant::now();
                    emit(&app);
                }
            })
            .await;
            {
                let mut i = lock(&inner);
                if !i.cancel.as_ref().is_some_and(|c| Arc::ptr_eq(c, &cancel)) {
                    return; // paused or superseded; whoever did that updated the state
                }
                i.cancel = None;
                match &result {
                    Ok(Fetched::Done) => {
                        i.state = "ready";
                        i.bytes_done = total_bytes(p);
                    }
                    Ok(Fetched::Cancelled) => return,
                    Err(e) => {
                        i.state = "failed";
                        i.error = Some(e.to_string());
                    }
                }
            }
            match result {
                Ok(_) => {
                    persist(&app, p, "ready");
                    super::worker::poke(&app);
                }
                Err(e) => {
                    eprintln!("[transcribe] {} pack failed: {e}", p.as_str());
                    persist(&app, p, "failed");
                }
            }
            emit(&app);
        });
        Ok(())
    }

    /// Pauses the pack's running download; `.part` files are kept for resuming.
    pub fn pause(&self, app: &AppHandle, p: Pack) {
        {
            let mut inner = lock(&self.slots[p.index()].inner);
            let Some(cancel) = inner.cancel.take() else { return };
            cancel.store(true, Ordering::SeqCst);
            inner.state = "paused";
        }
        persist(app, p, "paused");
        emit(app);
    }

    pub fn status(&self, app: &AppHandle) -> SpeechStatus {
        let paths = app.state::<Paths>();
        SpeechStatus {
            packs: PACKS
                .iter()
                .map(|&p| {
                    let (mut state, mut bytes_done, error) = {
                        let i = lock(&self.slots[p.index()].inner);
                        (i.state, i.bytes_done, i.error.clone())
                    };
                    if state == "none" && ready(&paths, p) {
                        state = "ready";
                    }
                    if state == "ready" {
                        bytes_done = total_bytes(p);
                    }
                    PackStatus {
                        id: p.as_str().to_string(),
                        state: state.to_string(),
                        bytes_done,
                        bytes_total: total_bytes(p),
                        error,
                    }
                })
                .collect(),
        }
    }

    /// Restores the persisted per-pack state; a download running when the app quit resumes.
    pub fn resume_if_needed(&self, app: &AppHandle) {
        let persisted = read_persisted(app);
        let paths = app.state::<Paths>().inner().clone();
        for p in PACKS {
            let Some(saved) = persisted.get(p.as_str()).and_then(Value::as_str) else { continue };
            {
                let mut i = lock(&self.slots[p.index()].inner);
                i.bytes_done = bytes_on_disk(&paths, p);
                i.state = match saved {
                    _ if ready(&paths, p) => "ready",
                    "downloading" | "paused" | "verifying" => "paused",
                    "failed" => "failed",
                    _ => "none",
                };
            }
            if matches!(saved, "downloading" | "verifying") {
                if let Err(e) = self.start(app, p) {
                    eprintln!("[transcribe] resume {} failed: {e}", p.as_str());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_ready_needs_every_file() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        std::fs::create_dir_all(&paths.models).unwrap();
        let [first, second] = &SPEECH;
        std::fs::write(model_path(&paths, first), b"x").unwrap();
        assert!(!ready(&paths, Pack::Speech));
        std::fs::write(model_path(&paths, second), b"x").unwrap();
        assert!(ready(&paths, Pack::Speech));
        assert!(!ready(&paths, Pack::Speaker));
    }

    #[test]
    fn pack_parses_known_ids_only() {
        assert_eq!("speech".parse::<Pack>().unwrap(), Pack::Speech);
        assert_eq!("speaker".parse::<Pack>().unwrap(), Pack::Speaker);
        assert!(matches!("other".parse::<Pack>(), Err(AppError::Code("invalid"))));
    }
}
