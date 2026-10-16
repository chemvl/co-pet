//! Capture pipeline (Step 6): a drop, the clipboard, a screenshot, a quick note or a selection
//! becomes items. Capture never waits on the network, extraction or AI and never opens a dialog;
//! the triage worker takes it from there.
pub mod clipboard;
pub mod drop;
pub mod screenshot;
pub mod selection;

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::db::{items, settings, Db};
use crate::error::AppResult;
use crate::events::{self, PetReaction};
use crate::i18n::{self, Lang};
use crate::library::{self, StoredFile};
use crate::model::{CaptureResult, NewItem};
use crate::paths::Paths;
use crate::platform::macos::frontmost::FrontApp;
use crate::platform::macos::pasteboard::PbContent;

const TITLE_MAX_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq)]
pub enum CaptureInput {
    /// One item per path; directories are skipped.
    Files { paths: Vec<PathBuf> },
    Url { url: String, title: Option<String> },
    Text { plain: String, html: Option<String> },
    /// PNG bytes. `url` is the link a browser image drag or copy carried along.
    Image { png_bytes: Vec<u8>, url: Option<String> },
    /// A finished `screencapture` file in `tmp/`; it is moved into the library.
    Screenshot { path: PathBuf },
    Note { text: String },
}

#[derive(Debug, Clone)]
pub struct CaptureSource {
    /// `drop | clipboard | screenshot | note | selection`
    pub kind: &'static str,
    pub app_name: Option<String>,
    pub bundle_id: Option<String>,
    /// `mcp_clients.id` when an agent added it (`items.added_by`).
    pub added_by: Option<String>,
    /// Replaces the title derived from the input (an agent's `add_item` title).
    pub title: Option<String>,
}

impl CaptureSource {
    pub fn new(kind: &'static str, app: Option<FrontApp>) -> Self {
        let (app_name, bundle_id) = app.map(|a| (a.name, a.bundle_id)).unwrap_or_default();
        CaptureSource { kind, app_name, bundle_id, added_by: None, title: None }
    }
}

pub(crate) enum Outcome {
    Saved(String),
    Duplicate(String),
    Skipped,
    Failed(String),
}

/// Stores the input, inserts the items and hands them to triage. Blocking (file copies, hashing):
/// async callers use [`capture_blocking`].
pub fn capture(app: &AppHandle, input: CaptureInput, source: CaptureSource) -> CaptureResult {
    let screenshot = matches!(input, CaptureInput::Screenshot { .. });
    let lang = i18n::lang(app);
    let outcomes = store(&app.state::<Db>(), &app.state::<Paths>(), lang, input, &source);
    finish(app, outcomes, screenshot)
}

/// The storage half of [`capture`]: blobs copied, rows inserted. No events, no triage.
pub(crate) fn store(db: &Db, paths: &Paths, lang: Lang, input: CaptureInput, source: &CaptureSource) -> Vec<Outcome> {
    let prepared: Vec<AppResult<Option<NewItem>>> = match input {
        CaptureInput::Files { paths: files } => {
            // An agent's file is never moved out of its folder.
            let remove_original = source.added_by.is_none()
                && settings::get::<bool>(&db.conn(), "capture.removeOriginal").unwrap_or(false);
            files.iter().map(|f| prepare_file(paths, f, remove_original, source)).collect()
        }
        other => vec![prepare(lang, paths, other, source)],
    };
    prepared
        .into_iter()
        .map(|p| match p {
            Ok(Some(mut new)) => {
                if let Some(title) = source.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                    new.title = title.to_owned();
                }
                commit(&mut db.conn(), paths, &new).unwrap_or_else(|e| Outcome::Failed(e.to_string()))
            }
            Ok(None) => Outcome::Skipped,
            Err(e) => Outcome::Failed(e.to_string()),
        })
        .collect()
}

/// [`capture`] on the blocking pool.
pub async fn capture_blocking(app: &AppHandle, input: CaptureInput, source: CaptureSource) -> CaptureResult {
    let handle = app.clone();
    match tauri::async_runtime::spawn_blocking(move || capture(&handle, input, source)).await {
        Ok(result) => result,
        Err(e) => failed(app, e.to_string()),
    }
}

/// The capture input for pasteboard content. `image` is the board's image data, preferred over a
/// URL so a browser image drag saves the picture (keeping the link).
pub(crate) fn input_from(content: PbContent, image: Option<Vec<u8>>) -> CaptureInput {
    match content {
        PbContent::Files(paths) => CaptureInput::Files { paths },
        PbContent::Url { url, title } => match image {
            Some(png_bytes) => CaptureInput::Image { png_bytes, url: Some(url) },
            None => CaptureInput::Url { url, title },
        },
        PbContent::Image(png_bytes) => CaptureInput::Image { png_bytes, url: None },
        PbContent::Text { plain, html } => CaptureInput::Text { plain, html },
    }
}

/// `skipped` with an optional `notice.*` message key and its parameters. No pet reaction: the
/// pet frontend reacts to the notice.
pub(crate) fn skipped(message: Option<&str>, params: &[(&str, String)]) -> CaptureResult {
    CaptureResult {
        status: "skipped",
        item_ids: Vec::new(),
        message: message.map(str::to_string),
        message_params: params.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
    }
}

/// `failed` with the raw reason; the pet plays `confused`.
pub(crate) fn failed(app: &AppHandle, reason: String) -> CaptureResult {
    eprintln!("[capture] failed: {reason}");
    events::pet_reaction(app, PetReaction::new("confused"));
    CaptureResult { status: "failed", message: Some(reason), ..CaptureResult::default() }
}

// ---------- preparing rows ----------

fn prepare_file(
    paths: &Paths,
    path: &Path,
    remove_original: bool,
    source: &CaptureSource,
) -> AppResult<Option<NewItem>> {
    if path.is_dir() {
        return Ok(None);
    }
    let stored = library::store_file(paths, path, remove_original)?;
    let kind = if stored.mime.as_deref().is_some_and(|m| m.starts_with("image/")) { "image" } else { "file" };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("item.{}", stored.ext));
    let mut new = blob_item(kind, name.clone(), &stored, source);
    new.file_name = Some(name);
    Ok(Some(new))
}

fn prepare(lang: Lang, paths: &Paths, input: CaptureInput, source: &CaptureSource) -> AppResult<Option<NewItem>> {
    let new = match input {
        CaptureInput::Files { .. } => unreachable!("files are prepared one by one"),
        CaptureInput::Url { url, title } => {
            let url = url.trim().to_string();
            if url.is_empty() {
                return Ok(None);
            }
            let title = title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty() && *t != url);
            NewItem {
                kind: "url".into(),
                title: title.unwrap_or_else(|| url_title(&url)),
                content_hash: hash_str(&format!("url:{url}")),
                url: Some(url),
                ..text_source(source)
            }
        }
        CaptureInput::Text { plain, html } => {
            if plain.trim().is_empty() {
                return Ok(None);
            }
            let html = html.filter(|h| !h.trim().is_empty());
            let kind = if looks_like_code(&plain) {
                "code"
            } else if html.is_some() {
                "richtext"
            } else {
                "text"
            };
            NewItem {
                kind: kind.into(),
                title: text_title(&plain),
                content_hash: hash_str(&format!("text:{}", plain.trim())),
                payload_text: Some(plain),
                payload_html: html,
                ..text_source(source)
            }
        }
        CaptureInput::Note { text } => {
            if text.trim().is_empty() {
                return Ok(None);
            }
            NewItem {
                kind: "text".into(),
                title: text_title(&text),
                content_hash: hash_str(&format!("text:{}", text.trim())),
                payload_text: Some(text),
                ..text_source(source)
            }
        }
        CaptureInput::Image { png_bytes, url } => {
            let stored = library::store_bytes(paths, &png_bytes, "png")?;
            let mut new = blob_item("image", timed_title(lang, "image.title"), &stored, source);
            new.url = url;
            new
        }
        CaptureInput::Screenshot { path } => {
            let stored = library::store_file(paths, &path, true)?;
            blob_item("screenshot", timed_title(lang, "screenshot.title"), &stored, source)
        }
    };
    Ok(Some(new))
}

fn blob_item(kind: &str, title: String, stored: &StoredFile, source: &CaptureSource) -> NewItem {
    NewItem {
        kind: kind.into(),
        title,
        file_path: Some(stored.rel_path.clone()),
        mime: stored.mime.clone(),
        size_bytes: Some(stored.size as i64),
        content_hash: stored.sha256.clone(),
        ..text_source(source)
    }
}

fn text_source(source: &CaptureSource) -> NewItem {
    NewItem {
        source_app: source.app_name.clone(),
        source_bundle_id: source.bundle_id.clone(),
        source_kind: source.kind.into(),
        added_by: source.added_by.clone(),
        ..NewItem::default()
    }
}

/// Inserts `new` unless a live item has the same content. A duplicate's freshly stored blob is
/// deleted when no row references it (same content under another extension).
fn commit(conn: &mut Connection, paths: &Paths, new: &NewItem) -> AppResult<Outcome> {
    let drop_blob = |conn: &Connection| -> AppResult<()> {
        if let Some(rel) = &new.file_path {
            if !items::file_path_in_use(conn, rel)? {
                library::remove_blob(paths, rel);
            }
        }
        Ok(())
    };
    if let Some(existing) = items::find_live_by_hash(conn, &new.content_hash)? {
        drop_blob(conn)?;
        return Ok(Outcome::Duplicate(existing));
    }
    match items::insert_item(conn, new) {
        Ok(id) => Ok(Outcome::Saved(id)),
        Err(e) => {
            drop_blob(conn)?;
            Err(e)
        }
    }
}

/// One result for the whole capture: any saved item wins, then duplicates, then the first
/// failure, else `skipped`. No events: [`finish`] adds them.
pub(crate) fn summarize(outcomes: Vec<Outcome>) -> CaptureResult {
    let (mut saved, mut duplicates, mut failures) = (Vec::new(), Vec::new(), Vec::new());
    for o in outcomes {
        match o {
            Outcome::Saved(id) => saved.push(id),
            Outcome::Duplicate(id) => duplicates.push(id),
            Outcome::Failed(reason) => failures.push(reason),
            Outcome::Skipped => {}
        }
    }
    for reason in &failures {
        eprintln!("[capture] item failed: {reason}");
    }
    if !saved.is_empty() {
        return CaptureResult { status: "saved", item_ids: saved, ..CaptureResult::default() };
    }
    if !duplicates.is_empty() {
        return CaptureResult { status: "duplicate", item_ids: duplicates, ..CaptureResult::default() };
    }
    match failures.into_iter().next() {
        Some(reason) => CaptureResult { status: "failed", message: Some(reason), ..CaptureResult::default() },
        None => skipped(None, &[]),
    }
}

/// [`summarize`] plus its pet reaction; saved items are announced and handed to triage.
fn finish(app: &AppHandle, outcomes: Vec<Outcome>, screenshot: bool) -> CaptureResult {
    let result = summarize(outcomes);
    match result.status {
        "saved" => {
            items::notify_changed(app, result.item_ids.clone());
            events::pet_reaction(app, PetReaction::new(if screenshot { "camera" } else { "catch" }));
            match app.try_state::<crate::triage::TriageQueue>() {
                Some(queue) => result.item_ids.iter().for_each(|id| queue.enqueue(id.clone())),
                // Still `pending`: the worker picks them up at its next start.
                None => eprintln!("[capture] triage queue not running; {} item(s) wait", result.item_ids.len()),
            }
        }
        "duplicate" => events::pet_reaction(app, PetReaction { variant: Some(3), ..PetReaction::new("fidget") }),
        "failed" => return failed(app, result.message.unwrap_or_default()),
        _ => {}
    }
    result
}

// ---------- pure helpers ----------

static CODE_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"[;{}]\s*$|^\s*(import|export|const|let|var|function|def|class|fn|pub|return|if|for|while|#include)\b|=>",
    )
    .expect("valid regex")
});

/// A ``` fence, or ≥2 non-empty lines of which ≥40% look like code.
pub fn looks_like_code(s: &str) -> bool {
    if s.trim_start().starts_with("```") {
        return true;
    }
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() < 2 {
        return false;
    }
    let code = lines.iter().filter(|l| CODE_LINE.is_match(l)).count();
    code * 10 >= lines.len() * 4
}

/// The first non-empty line (fence lines skipped), cut at 80 chars with `…`.
fn text_title(s: &str) -> String {
    let mut lines = s.lines().map(str::trim).filter(|l| !l.is_empty());
    let first = lines.clone().find(|l| !l.starts_with("```")).or_else(|| lines.next()).unwrap_or("");
    cut(first, TITLE_MAX_CHARS)
}

fn cut(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", s[..at].trim_end()),
        None => s.to_string(),
    }
}

/// The URL without its scheme (and without a bare trailing `/`), until extraction fetches a title.
fn url_title(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.trim_end_matches('/').to_string()
}

/// `Screenshot 14:05` / `Ảnh chụp 14:05` in the app language.
fn timed_title(lang: Lang, key: &str) -> String {
    format!("{} {}", i18n::tr(lang, key), chrono::Local::now().format("%H:%M"))
}

fn hash_str(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_detection() {
        assert!(looks_like_code("```rust\nfn main() {}\n```"));
        assert!(looks_like_code("const a = 1;\nconst b = 2;\nconsole.log(a + b)"));
        assert!(looks_like_code("def f(x):\n    return x * 2\n\nprint(f(2))"));
        assert!(looks_like_code("#include <stdio.h>\nint main() {\n  return 0;\n}"));
        assert!(looks_like_code("items.map(x =>\n  x.id)"));
        // One line is never enough, however code-like.
        assert!(!looks_like_code("const a = 1;"));
        // Prose: 1 of 3 lines ends in `;` (33% < 40%).
        assert!(!looks_like_code("Buy milk; eggs\nCall Lan about the API\nIf it rains, stay in"));
        assert!(!looks_like_code("Họp với Lan lúc 14:00 thứ Sáu\nvề API contract"));
    }

    #[test]
    fn titles_cut_at_80_chars() {
        let long = "a".repeat(100);
        assert_eq!(text_title(&long), format!("{}…", "a".repeat(80)));
        assert_eq!(text_title(&"a".repeat(80)), "a".repeat(80));
        let vi = "Đường ".repeat(20); // 120 chars, multi-byte
        let t = text_title(&vi);
        assert!(t.ends_with('…'));
        assert!(t.chars().count() <= 81);
        assert_eq!(text_title("\n\n  First line  \nsecond"), "First line");
        assert_eq!(text_title("```ts\nconst a = 1;\n```"), "const a = 1;");
        assert_eq!(text_title("```"), "```");
    }

    #[test]
    fn url_titles_drop_the_scheme() {
        assert_eq!(url_title("https://www.figma.com/file/abc"), "www.figma.com/file/abc");
        assert_eq!(url_title("http://example.com/"), "example.com");
    }

    #[test]
    fn browser_image_drag_prefers_the_image() {
        let url = PbContent::Url { url: "https://x.com/a.png".into(), title: None };
        assert_eq!(
            input_from(url.clone(), Some(vec![1, 2])),
            CaptureInput::Image { png_bytes: vec![1, 2], url: Some("https://x.com/a.png".into()) }
        );
        assert_eq!(input_from(url, None), CaptureInput::Url { url: "https://x.com/a.png".into(), title: None });
    }
}
