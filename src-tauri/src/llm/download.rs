//! On-demand model download: resumable (`Range`), throttled `ai-status` progress, sha256
//! verification, and `ai.download` persistence so an interrupted download resumes at startup.
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use reqwest::header::RANGE;
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};
use tokio::io::AsyncWriteExt;

use crate::db::{settings, Db};
use crate::error::{AppError, AppResult};
use crate::events;
use crate::llm::models::{self, ModelSpec, REGISTRY};
use crate::model::{AiStatus, ModelInfo};
use crate::paths::Paths;

const EMIT_EVERY: Duration = Duration::from_millis(500);
const DOWNLOAD_SETTING: &str = "ai.download";

/// Progress reported by [`fetch_model`].
pub enum Progress {
    /// Bytes present in the `.part` file so far.
    Bytes(u64),
    Verifying,
}

pub enum Fetched {
    Done,
    Cancelled,
}

/// Value of the `ai.download` setting.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Persisted {
    model_id: String,
    state: String,
}

struct Inner {
    model_id: Option<&'static str>,
    /// `none | downloading | paused | verifying | ready | failed`
    state: &'static str,
    bytes_done: u64,
    error: Option<String>,
    /// Cancel flag of the current transfer task; identifies it as current.
    cancel: Option<Arc<AtomicBool>>,
}

/// Managed state (`app.state::<Downloader>()`).
pub struct Downloader {
    inner: Arc<Mutex<Inner>>,
    /// Held by a transfer task for its whole run, so a restarted download of the same model
    /// never appends to the `.part` file while the cancelled task is still writing it.
    transfer: Arc<tokio::sync::Mutex<()>>,
}

impl Default for Downloader {
    fn default() -> Self {
        Downloader {
            inner: Arc::new(Mutex::new(Inner {
                model_id: None,
                state: "none",
                bytes_done: 0,
                error: None,
                cancel: None,
            })),
            transfer: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}

fn lock(inner: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    inner.lock().unwrap_or_else(|e| e.into_inner())
}

fn file_len(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

fn persist(app: &AppHandle, model_id: &str, state: &str) {
    let v = json!({ "modelId": model_id, "state": state });
    if let Err(e) = settings::set(app, DOWNLOAD_SETTING, &v) {
        eprintln!("[download] persist {DOWNLOAD_SETTING} failed: {e}");
    }
}

/// Makes a freshly downloaded model the active one unless a usable model is already selected.
fn activate(app: &AppHandle, spec: &ModelSpec) {
    let paths = app.state::<Paths>();
    let current = settings::read::<Option<String>>(app, "ai.modelId").ok().flatten();
    let usable = current
        .as_deref()
        .and_then(models::find)
        .is_some_and(|m| models::model_path(&paths, m).exists());
    if !usable {
        if let Err(e) = settings::set(app, "ai.modelId", &Value::String(spec.id.into())) {
            eprintln!("[download] set ai.modelId failed: {e}");
        }
    }
}

impl Downloader {
    /// Starts (or resumes) downloading `model_id`. Switching to another model cancels the
    /// current transfer and deletes its `.part`. Errors: `localOnly`, `unknownModel`.
    pub fn start(&self, app: &AppHandle, model_id: &str) -> AppResult<()> {
        if settings::read::<bool>(app, "privacy.localOnly")? {
            return Err(AppError::Code("localOnly"));
        }
        let spec = models::find(model_id).ok_or(AppError::Code("unknownModel"))?;
        let paths = app.state::<Paths>().inner().clone();
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut inner = lock(&self.inner);
            if let Some(old) = inner.cancel.take() {
                old.store(true, Ordering::SeqCst);
            }
            if let Some(prev) = inner.model_id.filter(|p| *p != spec.id).and_then(models::find) {
                let _ = std::fs::remove_file(models::part_path(&paths, prev));
            }
            inner.model_id = Some(spec.id);
            inner.error = None;
            if models::model_path(&paths, spec).exists() {
                inner.state = "ready";
                inner.bytes_done = spec.bytes;
            } else {
                inner.state = "downloading";
                inner.bytes_done = file_len(&models::part_path(&paths, spec));
                inner.cancel = Some(cancel.clone());
            }
        }
        if lock(&self.inner).state == "ready" {
            persist(app, spec.id, "ready");
            activate(app, spec);
            crate::transcribe::worker::llm_model_ready(app);
            events::ai_status(app, self.status(app));
            return Ok(());
        }
        persist(app, spec.id, "downloading");
        events::ai_status(app, self.status(app));

        let app = app.clone();
        let inner = self.inner.clone();
        let transfer = self.transfer.clone();
        tauri::async_runtime::spawn(async move {
            let _guard = transfer.lock().await;
            let mut last_emit = Instant::now();
            let result = fetch_model(&paths, spec, &cancel, |p| {
                {
                    let mut i = lock(&inner);
                    if !i.cancel.as_ref().is_some_and(|c| Arc::ptr_eq(c, &cancel)) {
                        return;
                    }
                    match p {
                        Progress::Bytes(n) => i.bytes_done = n,
                        Progress::Verifying => i.state = "verifying",
                    }
                }
                if matches!(p, Progress::Verifying) || last_emit.elapsed() >= EMIT_EVERY {
                    last_emit = Instant::now();
                    events::ai_status(&app, app.state::<Downloader>().status(&app));
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
                        i.bytes_done = spec.bytes;
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
                    persist(&app, spec.id, "ready");
                    activate(&app, spec);
                    crate::transcribe::worker::llm_model_ready(&app);
                }
                Err(e) => {
                    eprintln!("[download] {} failed: {e}", spec.id);
                    persist(&app, spec.id, "failed");
                }
            }
            events::ai_status(&app, app.state::<Downloader>().status(&app));
        });
        Ok(())
    }

    /// Pauses the running download; the `.part` file is kept for resuming.
    pub fn pause(&self, app: &AppHandle) {
        let model_id = {
            let mut inner = lock(&self.inner);
            let Some(cancel) = inner.cancel.take() else { return };
            cancel.store(true, Ordering::SeqCst);
            inner.state = "paused";
            inner.model_id
        };
        if let Some(id) = model_id {
            persist(app, id, "paused");
        }
        events::ai_status(app, self.status(app));
    }

    pub fn status(&self, app: &AppHandle) -> AiStatus {
        let paths = app.state::<Paths>();
        let (mut model_id, mut state, mut bytes_done, error) = {
            let i = lock(&self.inner);
            (i.model_id, i.state, i.bytes_done, i.error.clone())
        };
        if model_id.is_none() {
            model_id = settings::read::<Option<String>>(app, "ai.modelId")
                .ok()
                .flatten()
                .as_deref()
                .and_then(models::find)
                .map(|m| m.id);
        }
        let spec = model_id.and_then(models::find);
        if state == "none" && spec.is_some_and(|m| models::model_path(&paths, m).exists()) {
            state = "ready";
        }
        if state == "ready" {
            bytes_done = spec.map_or(0, |m| m.bytes);
        }
        AiStatus {
            model_id: model_id.map(String::from),
            state: state.to_string(),
            bytes_done,
            bytes_total: spec.map_or(0, |m| m.bytes),
            error,
            models: REGISTRY
                .iter()
                .map(|m| ModelInfo {
                    id: m.id.to_string(),
                    label: m.label.to_string(),
                    bytes: m.bytes,
                    downloaded: models::model_path(&paths, m).exists(),
                })
                .collect(),
            recommended_id: models::recommended_model().id.to_string(),
        }
    }

    /// Restores the persisted download state; a download that was running when the app quit
    /// resumes.
    pub fn resume_if_needed(&self, app: &AppHandle) {
        // `ai.download` is not seeded, so it may be absent.
        let persisted = settings::get_value(&app.state::<Db>().conn(), DOWNLOAD_SETTING)
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_value::<Persisted>(v).ok());
        let Some(p) = persisted else { return };
        let Some(spec) = models::find(&p.model_id) else { return };
        let paths = app.state::<Paths>();
        {
            let mut i = lock(&self.inner);
            i.model_id = Some(spec.id);
            i.bytes_done = file_len(&models::part_path(&paths, spec));
            i.state = match p.state.as_str() {
                _ if models::model_path(&paths, spec).exists() => "ready",
                "downloading" | "paused" | "verifying" => "paused",
                "failed" => "failed",
                _ => "none",
            };
        }
        if matches!(p.state.as_str(), "downloading" | "verifying") {
            if let Err(e) = self.start(app, spec.id) {
                eprintln!("[download] resume {} failed: {e}", spec.id);
            }
        }
    }
}

/// Downloads `spec` into `models/<file>.part` (resuming with `Range` when a part exists;
/// a `200` reply restarts from zero), verifies sha256 and renames it to `models/<file>`.
/// A checksum mismatch deletes the part and fails with `checksum`.
pub async fn fetch_model(
    paths: &Paths,
    spec: &'static ModelSpec,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(Progress),
) -> AppResult<Fetched> {
    let part = models::part_path(paths, spec);
    tokio::fs::create_dir_all(&paths.models).await?;
    let mut have = file_len(&part);
    if have < spec.bytes {
        let client = reqwest::Client::builder()
            .user_agent("Co-pet/0.1")
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(60))
            .build()?;
        let mut req = client.get(spec.url);
        if have > 0 {
            req = req.header(RANGE, format!("bytes={have}-"));
        }
        let mut resp = req.send().await?;
        let status = resp.status();
        let file = if status == StatusCode::PARTIAL_CONTENT {
            Some(tokio::fs::OpenOptions::new().append(true).open(&part).await?)
        } else if status.is_success() {
            have = 0;
            Some(tokio::fs::File::create(&part).await?)
        } else if status == StatusCode::RANGE_NOT_SATISFIABLE {
            None // the part already holds every byte; verification decides
        } else {
            return Err(AppError::msg(format!("HTTP {status}")));
        };
        if let Some(file) = file {
            let mut out = tokio::io::BufWriter::with_capacity(1 << 20, file);
            on_progress(Progress::Bytes(have));
            while let Some(chunk) = resp.chunk().await? {
                if cancel.load(Ordering::SeqCst) {
                    out.flush().await?;
                    return Ok(Fetched::Cancelled);
                }
                out.write_all(&chunk).await?;
                have += chunk.len() as u64;
                on_progress(Progress::Bytes(have));
            }
            out.flush().await?;
        }
    }
    if cancel.load(Ordering::SeqCst) {
        return Ok(Fetched::Cancelled);
    }
    on_progress(Progress::Verifying);
    let hash_path = part.clone();
    let digest = tokio::task::spawn_blocking(move || -> std::io::Result<String> {
        let mut f = std::fs::File::open(hash_path)?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = std::io::Read::read(&mut f, &mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        Ok(hex::encode(h.finalize()))
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))??;
    if digest != spec.sha256 {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(AppError::Code("checksum"));
    }
    tokio::fs::rename(&part, models::model_path(paths, spec)).await?;
    Ok(Fetched::Done)
}
