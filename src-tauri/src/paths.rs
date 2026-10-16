//! App data layout under `~/Library/Application Support/com.copet.app/`.
use std::path::{Path, PathBuf};

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
}

impl Paths {
    pub fn new(root: PathBuf) -> Self {
        Paths {
            db: root.join("copet.db"),
            library: root.join("library"),
            thumbs: root.join("library").join("thumbs"),
            models: root.join("models"),
            tmp: root.join("tmp"),
            root,
        }
    }

    pub fn resolve(app: &AppHandle) -> AppResult<Self> {
        Ok(Self::new(app.path().app_data_dir()?))
    }

    /// Creates every folder that must exist.
    pub fn ensure(&self) -> AppResult<()> {
        for d in [&self.root, &self.library, &self.thumbs, &self.models, &self.tmp] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
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

    pub fn rel<'a>(&self, abs: &'a Path) -> Option<&'a Path> {
        abs.strip_prefix(&self.library).ok()
    }
}
