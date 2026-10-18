//! App data layout under the Tauri app data folder (`~/Library/Application Support/com.copet.app/`
//! on macOS, `%APPDATA%\com.copet.app\` on Windows).
use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use crate::error::AppResult;

#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
    pub db: PathBuf,
    pub library: PathBuf,
    pub thumbs: PathBuf,
    pub models: PathBuf,
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
            db: root.join("copet.db"),
            library: root.join("library"),
            thumbs: root.join("library").join("thumbs"),
            models: root.join("models"),
            tmp: root.join("tmp"),
            recordings: root.join("recordings"),
            #[cfg(unix)]
            sock: root.join("mcp.sock"),
            root,
        }
    }

    pub fn resolve(app: &AppHandle) -> AppResult<Self> {
        Ok(Self::new(app.path().app_data_dir()?))
    }

    /// Creates every folder that must exist.
    pub fn ensure(&self) -> AppResult<()> {
        for d in [&self.root, &self.library, &self.thumbs, &self.models, &self.tmp, &self.recordings] {
            std::fs::create_dir_all(d)?;
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
