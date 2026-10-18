//! App data layout. `base` is the Tauri app data folder (`~/Library/Application Support/com.copet.app/`
//! on macOS, `%APPDATA%\com.copet.app\` on Windows) and holds `locations.json`, which can point
//! the data folder (`root`) and the models folder elsewhere (see `storage.rs`).
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::error::AppResult;

#[derive(Debug, Clone)]
pub struct Paths {
    /// The Tauri app data folder: `locations.json` and the default data and models folders.
    pub base: PathBuf,
    pub root: PathBuf,
    pub db: PathBuf,
    pub library: PathBuf,
    pub thumbs: PathBuf,
    /// Swapped live by a models-folder move; clones share it.
    models: Arc<RwLock<PathBuf>>,
    pub tmp: PathBuf,
    /// Meeting audio, one folder per meeting (`recordings/<meetingId>/`). Never synced.
    pub recordings: PathBuf,
    /// Unix socket the `copet-mcp` sidecar talks to (macOS).
    #[cfg(unix)]
    pub sock: PathBuf,
}

impl Paths {
    pub fn new(root: PathBuf) -> Self {
        Paths {
            base: root.clone(),
            db: root.join("copet.db"),
            library: root.join("library"),
            thumbs: root.join("library").join("thumbs"),
            models: Arc::new(RwLock::new(root.join("models"))),
            tmp: root.join("tmp"),
            recordings: root.join("recordings"),
            #[cfg(unix)]
            sock: root.join("mcp.sock"),
            root,
        }
    }

    /// The folder holding the local models.
    pub fn models(&self) -> PathBuf {
        self.models.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub(crate) fn set_models(&self, dir: PathBuf) {
        *self.models.write().unwrap_or_else(|e| e.into_inner()) = dir;
    }

    /// Creates every folder that must exist. The models folder is best-effort: an unplugged
    /// models drive must not stop startup (downloads then fail with the IO error).
    pub fn ensure(&self) -> AppResult<()> {
        for d in [&self.root, &self.library, &self.thumbs, &self.tmp, &self.recordings] {
            std::fs::create_dir_all(d)?;
        }
        let models = self.models();
        if let Err(e) = std::fs::create_dir_all(&models) {
            eprintln!("[paths] models folder {}: {e}", models.display());
        }
        Ok(())
    }

    /// The audio folder of one meeting.
    pub fn recording_dir(&self, meeting_id: &str) -> PathBuf {
        self.recordings.join(meeting_id)
    }

    /// Empties `tmp/` (called once at startup).
    pub fn clear_tmp(&self) -> AppResult<()> {
        if self.tmp.exists() {
            std::fs::remove_dir_all(&self.tmp)?;
        }
        std::fs::create_dir_all(&self.tmp)?;
        Ok(())
    }

    /// Absolute path of a library-relative path such as `ab/abcd….pdf` or `thumbs/abcd….png`.
    pub fn abs(&self, rel: &str) -> PathBuf {
        self.library.join(rel)
    }

    /// Where the `copet-mcp` sidecar connects: the Unix socket (macOS) or the per-user named
    /// pipe (Windows). Passed to the sidecar as `COPET_SOCKET`.
    pub fn mcp_endpoint(&self) -> String {
        #[cfg(unix)]
        return self.sock.to_string_lossy().into_owned();
        #[cfg(windows)]
        return crate::agent::pipe::pipe_name(&std::env::var("USERNAME").unwrap_or_default());
    }
}
