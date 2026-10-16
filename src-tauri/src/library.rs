//! Content-addressed blob store: `library/<hh>/<sha256>.<ext>`, thumbnails in
//! `library/thumbs/<sha256>.png`, drag-out staging under `tmp/dragout/<uuid>/`.
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::db::new_id;
use crate::error::{AppError, AppResult};
use crate::model::ItemRow;
use crate::paths::Paths;

const THUMB_SIDE: u32 = 256;
const THUMB_MAX_SOURCE: u64 = 40 * 1024 * 1024;
/// Bytes kept from the start of a file for MIME sniffing.
const SNIFF_LEN: usize = 8192;

#[derive(Debug, Clone)]
pub struct StoredFile {
    /// Relative to `Paths::library`.
    pub rel_path: String,
    pub sha256: String,
    pub size: u64,
    /// Sniffed from the content; `None` when the format has no magic number (plain text, …).
    pub mime: Option<String>,
    /// Lowercase original extension, or `bin`.
    pub ext: String,
}

/// Lowercase alphanumeric extension of `path`, or `bin`.
fn ext_of(path: &Path) -> String {
    clean_ext(path.extension().and_then(|e| e.to_str()).unwrap_or(""))
}

fn clean_ext(ext: &str) -> String {
    let ext = ext.to_ascii_lowercase();
    if ext.is_empty() || ext.len() > 16 || !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        "bin".into()
    } else {
        ext
    }
}

fn blob_rel(sha: &str, ext: &str) -> String {
    format!("{}/{sha}.{ext}", &sha[..2])
}

/// A fresh `tmp/` file path for staging a write.
pub(crate) fn staging(paths: &Paths) -> AppResult<PathBuf> {
    fs::create_dir_all(&paths.tmp)?;
    Ok(paths.tmp.join(format!("store-{}.part", new_id())))
}

/// Moves a staged file to `rel` (creating its folder) unless identical content is already there.
pub(crate) fn commit(paths: &Paths, staged: &Path, rel: &str) -> AppResult<()> {
    let dest = paths.abs(rel);
    if dest.exists() {
        fs::remove_file(staged)?;
        return Ok(());
    }
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::rename(staged, &dest)?;
    Ok(())
}

/// Copies `src` into the library, hashing while streaming. With `move_original`, the source is
/// removed after a successful store; failing to remove it is logged, not fatal.
pub fn store_file(paths: &Paths, src: &Path, move_original: bool) -> AppResult<StoredFile> {
    if src.is_dir() {
        return Err(AppError::Code("isDirectory"));
    }
    let staged = staging(paths)?;
    let result = (|| -> AppResult<(String, u64, Vec<u8>)> {
        let mut input = File::open(src)?;
        let mut out = File::create(&staged)?;
        let mut hasher = Sha256::new();
        let mut head = Vec::with_capacity(SNIFF_LEN);
        let mut buf = vec![0u8; 64 * 1024];
        let mut size = 0u64;
        loop {
            let n = input.read(&mut buf)?;
            if n == 0 {
                break;
            }
            let chunk = &buf[..n];
            hasher.update(chunk);
            out.write_all(chunk)?;
            if head.len() < SNIFF_LEN {
                let take = (SNIFF_LEN - head.len()).min(n);
                head.extend_from_slice(&chunk[..take]);
            }
            size += n as u64;
        }
        out.sync_all()?;
        Ok((hex::encode(hasher.finalize()), size, head))
    })();
    let (sha, size, head) = match result {
        Ok(v) => v,
        Err(e) => {
            let _ = fs::remove_file(&staged);
            return Err(e);
        }
    };
    let ext = ext_of(src);
    let rel = blob_rel(&sha, &ext);
    commit(paths, &staged, &rel)?;
    if move_original {
        if let Err(e) = fs::remove_file(src) {
            eprintln!("[library] could not remove original {}: {e}", src.display());
        }
    }
    Ok(StoredFile {
        rel_path: rel,
        sha256: sha,
        size,
        mime: infer::get(&head).map(|t| t.mime_type().to_string()),
        ext,
    })
}

/// Stores in-memory bytes (pasted images, downloaded previews) under the given extension.
pub fn store_bytes(paths: &Paths, bytes: &[u8], ext: &str) -> AppResult<StoredFile> {
    let sha = hex::encode(Sha256::digest(bytes));
    let ext = clean_ext(ext);
    let rel = blob_rel(&sha, &ext);
    if !paths.abs(&rel).exists() {
        let staged = staging(paths)?;
        fs::write(&staged, bytes)?;
        commit(paths, &staged, &rel)?;
    }
    Ok(StoredFile {
        rel_path: rel,
        sha256: sha,
        size: bytes.len() as u64,
        mime: infer::get(bytes).map(|t| t.mime_type().to_string()),
        ext,
    })
}

/// A PNG thumbnail (longest side 256 px) of an image blob, as `thumbs/<sha>.png`. `None` for
/// files over 40 MB or formats the `image` crate cannot decode.
pub fn make_thumb(paths: &Paths, rel: &str) -> Option<String> {
    let src = paths.abs(rel);
    let sha = src.file_stem()?.to_str()?;
    let thumb_rel = format!("thumbs/{sha}.png");
    if paths.abs(&thumb_rel).exists() {
        return Some(thumb_rel);
    }
    if fs::metadata(&src).ok()?.len() > THUMB_MAX_SOURCE {
        return None;
    }
    let img = match image::open(&src) {
        Ok(img) => img,
        Err(e) => {
            eprintln!("[library] thumbnail decode failed for {rel}: {e}");
            return None;
        }
    };
    let img = if img.width() > THUMB_SIDE || img.height() > THUMB_SIDE {
        img.thumbnail(THUMB_SIDE, THUMB_SIDE)
    } else {
        img
    };
    let staged = paths.tmp.join(format!("thumb-{}.png", new_id()));
    let saved = fs::create_dir_all(&paths.tmp)
        .map_err(AppError::from)
        .and_then(|_| {
            img.save_with_format(&staged, image::ImageFormat::Png)
                .map_err(|e| AppError::msg(e.to_string()))
        })
        .and_then(|_| commit(paths, &staged, &thumb_rel));
    match saved {
        Ok(()) => Some(thumb_rel),
        Err(e) => {
            let _ = fs::remove_file(&staged);
            eprintln!("[library] thumbnail write failed for {rel}: {e}");
            None
        }
    }
}

/// Deletes a library file; a missing file is fine, other failures are logged.
pub fn remove_blob(paths: &Paths, rel: &str) {
    match fs::remove_file(paths.abs(rel)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => eprintln!("[library] could not remove {rel}: {e}"),
    }
}

/// A file name safe to create in a folder: no path separators, no leading dot, never empty.
fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | ':' | '\\' | '\0') || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.');
    if trimmed.is_empty() {
        return "item".into();
    }
    // APFS NAME_MAX is 255 bytes; keep ≤200 bytes, truncating the stem on a char boundary.
    const MAX: usize = 200;
    if trimmed.len() <= MAX {
        return trimmed.into();
    }
    let (stem, ext) = match trimmed.rfind('.') {
        Some(i) if trimmed.len() - i <= 16 => trimmed.split_at(i),
        _ => (trimmed, ""),
    };
    let mut end = MAX - ext.len();
    while !stem.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{ext}", &stem[..end])
}

fn dragout_dir(paths: &Paths) -> AppResult<PathBuf> {
    let dir = paths.tmp.join("dragout").join(new_id());
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// The blob under its original file name in `tmp/dragout/<uuid>/`, so a drop keeps the real
/// name. Hard-linked; copied when linking fails.
pub fn dragout_path(paths: &Paths, item: &ItemRow) -> AppResult<PathBuf> {
    let rel = item.file_path.as_deref().ok_or(AppError::Code("noFile"))?;
    let src = paths.abs(rel);
    let name = match &item.file_name {
        Some(n) => safe_name(n),
        None => {
            let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("bin");
            format!("{}.{ext}", safe_name(&item.title))
        }
    };
    let dest = dragout_dir(paths)?.join(name);
    if fs::hard_link(&src, &dest).is_err() {
        fs::copy(&src, &dest)?;
    }
    Ok(dest)
}

/// Writes `<name>.<ext>` (e.g. a `.md` or `.webloc`) into a fresh `tmp/dragout/<uuid>/`.
pub fn temp_text_file(paths: &Paths, name: &str, ext: &str, content: &str) -> AppResult<PathBuf> {
    let dest = dragout_dir(paths)?.join(format!("{}.{}", safe_name(name), clean_ext(ext)));
    fs::write(&dest, content)?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_name_truncates_bytes_keeping_extension() {
        let long = format!("{}.pdf", "Đường đến Việt Nam ".repeat(20));
        let name = safe_name(&long);
        assert!(name.len() <= 200);
        assert!(name.ends_with(".pdf"));
        assert!(name.starts_with("Đường"));
        let plain = safe_name(&"Việt".repeat(80));
        assert!(plain.len() <= 200 && plain.starts_with("Việt"));
    }

    fn paths() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure().unwrap();
        (dir, paths)
    }

    #[test]
    fn store_file_is_content_addressed_and_moves_original() {
        let (dir, paths) = paths();
        let src = dir.path().join("Report.PDF");
        fs::write(&src, b"%PDF-1.4 hello").unwrap();
        let stored = store_file(&paths, &src, true).unwrap();
        let sha = hex::encode(Sha256::digest(b"%PDF-1.4 hello"));
        assert_eq!(stored.rel_path, format!("{}/{sha}.pdf", &sha[..2]));
        assert_eq!((stored.size, stored.ext.as_str()), (14, "pdf"));
        assert_eq!(stored.mime.as_deref(), Some("application/pdf"));
        assert_eq!(
            fs::read(paths.abs(&stored.rel_path)).unwrap(),
            b"%PDF-1.4 hello"
        );
        assert!(!src.exists());
        assert_eq!(fs::read_dir(&paths.tmp).unwrap().count(), 0);

        let again = store_bytes(&paths, b"%PDF-1.4 hello", "pdf").unwrap();
        assert_eq!(again.rel_path, stored.rel_path);
    }

    #[test]
    fn thumb_fits_256() {
        let (_dir, paths) = paths();
        let mut png = Vec::new();
        image::DynamicImage::new_rgb8(1000, 500)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let stored = store_bytes(&paths, &png, "png").unwrap();
        let thumb = make_thumb(&paths, &stored.rel_path).unwrap();
        assert_eq!(thumb, format!("thumbs/{}.png", stored.sha256));
        let img = image::open(paths.abs(&thumb)).unwrap();
        assert_eq!((img.width(), img.height()), (256, 128));
    }

    #[test]
    fn dragout_keeps_original_name() {
        let (_dir, paths) = paths();
        let stored = store_bytes(&paths, b"hello", "txt").unwrap();
        let item = ItemRow {
            file_path: Some(stored.rel_path),
            file_name: Some("notes/today.txt".into()),
            ..Default::default()
        };
        let out = dragout_path(&paths, &item).unwrap();
        assert_eq!(out.file_name().unwrap(), "notes-today.txt");
        assert_eq!(fs::read(out).unwrap(), b"hello");
    }
}
