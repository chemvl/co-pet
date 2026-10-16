//! Settings › Capture and Settings › Data (Step 12): excluded apps, export and "Delete
//! everything"; saving text (the recovery key) to a file the user picked.
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSBundle, NSString};
use rusqlite::types::ValueRef;
use rusqlite::Connection;
use serde_json::{Map, Value};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;
use unicode_normalization::UnicodeNormalization;

use crate::account;
use crate::db::{settings, Db};
use crate::error::{AppError, AppResult};
use crate::llm::LlmServer;
use crate::model::ExcludedApp;
use crate::paths::Paths;
use crate::secrets;
use crate::windows;

const EXCLUDED_APPS: &str = "capture.excludedApps";
/// Accepted after trimming, NFC and lowercasing (`xoá` is the older Vietnamese spelling).
const CONFIRMATIONS: [&str; 3] = ["delete everything", "xóa tất cả", "xoá tất cả"];

// ---------- excluded apps ----------

/// Lets the user pick an `.app` in /Applications and appends its bundle id to
/// `capture.excludedApps`. `None` when cancelled; error `notAnApp` without a bundle id.
pub async fn pick_excluded_app(app: &AppHandle) -> AppResult<Option<String>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app
        .dialog()
        .file()
        .set_directory("/Applications")
        .add_filter("Applications", &["app"]);
    if let Some(home) = app.get_webview_window(windows::HOME) {
        dialog = dialog.set_parent(&home);
    }
    dialog.pick_file(move |picked| {
        let _ = tx.send(picked);
    });
    let Some(picked) = rx.await.ok().flatten() else { return Ok(None) };
    let path = picked.into_path().map_err(|e| AppError::msg(e.to_string()))?;
    let id = bundle_id(&path).ok_or(AppError::Code("notAnApp"))?;
    let mut ids: Vec<String> = settings::read(app, EXCLUDED_APPS)?;
    if !ids.contains(&id) {
        ids.push(id.clone());
        settings::set(app, EXCLUDED_APPS, &serde_json::to_value(ids)?)?;
    }
    Ok(Some(id))
}

fn bundle_id(app_path: &Path) -> Option<String> {
    let bundle = NSBundle::bundleWithPath(&NSString::from_str(app_path.to_str()?))?;
    Some(bundle.bundleIdentifier()?.to_string())
}

/// Every excluded bundle id with the installed app's display name (`None` when not installed).
pub fn list_excluded_apps(app: &AppHandle) -> AppResult<Vec<ExcludedApp>> {
    let ids: Vec<String> = settings::read(app, EXCLUDED_APPS)?;
    let workspace = NSWorkspace::sharedWorkspace();
    Ok(ids
        .into_iter()
        .map(|bundle_id| {
            let name = workspace
                .URLForApplicationWithBundleIdentifier(&NSString::from_str(&bundle_id))
                .and_then(|url| app_name(&url));
            ExcludedApp { bundle_id, name }
        })
        .collect())
}

/// `CFBundleDisplayName`, else `CFBundleName`, else the bundle's file name without `.app`.
fn app_name(url: &objc2_foundation::NSURL) -> Option<String> {
    if let Some(bundle) = NSBundle::bundleWithURL(url) {
        for key in ["CFBundleDisplayName", "CFBundleName"] {
            let value = bundle.objectForInfoDictionaryKey(&NSString::from_str(key));
            if let Some(name) = value.and_then(|v| v.downcast::<NSString>().ok()) {
                let name = name.to_string();
                if !name.trim().is_empty() {
                    return Some(name);
                }
            }
        }
    }
    let path = url.path()?.to_string();
    Path::new(&path).file_stem().map(|s| s.to_string_lossy().into_owned())
}

// ---------- export ----------

/// Writes `<dir>/Co-pet export <YYYY-MM-DD HHmm>/` with `items.json` (every column of every
/// non-deleted item), `tasks.json`, `rules.json`, `categories.json` and `files/` (copies of stored files).
/// Returns the folder.
pub fn export_all(app: &AppHandle, dir: &Path) -> AppResult<PathBuf> {
    if !dir.is_dir() {
        return Err(AppError::Code("notFound"));
    }
    let paths = app.state::<Paths>();
    let (items, tasks, rules, categories) = {
        let db = app.state::<Db>();
        let conn = db.conn();
        (
            rows_json(&conn, "SELECT * FROM items WHERE deleted_at IS NULL ORDER BY captured_at")?,
            rows_json(&conn, "SELECT * FROM tasks WHERE deleted_at IS NULL ORDER BY created_at")?,
            rows_json(&conn, "SELECT * FROM rules WHERE deleted_at IS NULL ORDER BY builtin, position")?,
            rows_json(&conn, "SELECT * FROM categories WHERE deleted_at IS NULL ORDER BY position, id")?,
        )
    };

    let stamp = chrono::Local::now().format("%Y-%m-%d %H%M").to_string();
    let out = unique_dir(dir, &format!("Co-pet export {stamp}"));
    let files = out.join("files");
    std::fs::create_dir_all(&files)?;
    std::fs::write(out.join("items.json"), serde_json::to_vec_pretty(&items)?)?;
    std::fs::write(out.join("tasks.json"), serde_json::to_vec_pretty(&tasks)?)?;
    std::fs::write(out.join("rules.json"), serde_json::to_vec_pretty(&rules)?)?;
    std::fs::write(out.join("categories.json"), serde_json::to_vec_pretty(&categories)?)?;

    let mut taken = HashSet::new();
    for item in &items {
        let Some(rel) = item["file_path"].as_str() else { continue };
        let src = paths.abs(rel);
        let name = export_name(item, rel);
        let dest = unique_name(&files, &name, &mut taken);
        if let Err(e) = std::fs::copy(&src, &dest) {
            eprintln!("[export] {} → {}: {e}", src.display(), dest.display());
        }
    }
    Ok(out)
}

/// One JSON object per row, keyed by column name.
fn rows_json(conn: &Connection, sql: &str) -> AppResult<Vec<Value>> {
    let mut stmt = conn.prepare(sql)?;
    let names: Vec<String> = stmt.column_names().into_iter().map(String::from).collect();
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut obj = Map::new();
        for (i, name) in names.iter().enumerate() {
            let v = match row.get_ref(i)? {
                ValueRef::Null => Value::Null,
                ValueRef::Integer(n) => Value::from(n),
                ValueRef::Real(f) => Value::from(f),
                ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
                ValueRef::Blob(b) => Value::String(hex::encode(b)),
            };
            obj.insert(name.clone(), v);
        }
        out.push(Value::Object(obj));
    }
    Ok(out)
}

/// The original file name, else `<id>.<ext>` from the stored blob; never a path.
fn export_name(item: &Value, rel: &str) -> String {
    let name = item["file_name"].as_str().map(str::trim).filter(|n| !n.is_empty());
    let name = match name {
        Some(n) => n.to_string(),
        None => {
            let id = item["id"].as_str().unwrap_or("item");
            let ext = Path::new(rel).extension().and_then(|e| e.to_str()).unwrap_or("bin");
            format!("{id}.{ext}")
        }
    };
    name.replace(['/', ':'], "_")
}

/// `name`, or `stem (2).ext`, `stem (3).ext`, … — unique in `dir` and in `taken` (compared
/// case-insensitively, like APFS).
fn unique_name(dir: &Path, name: &str, taken: &mut HashSet<String>) -> PathBuf {
    let p = Path::new(name);
    let stem = p.file_stem().map_or(name.to_string(), |s| s.to_string_lossy().into_owned());
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let mut n = 1;
    loop {
        let candidate = if n == 1 { name.to_string() } else { format!("{stem} ({n}){ext}") };
        if taken.insert(candidate.to_lowercase()) {
            return dir.join(candidate);
        }
        n += 1;
    }
}

/// `dir/base`, or `dir/base (2)` … when it already exists.
fn unique_dir(dir: &Path, base: &str) -> PathBuf {
    let mut n = 1;
    loop {
        let name = if n == 1 { base.to_string() } else { format!("{base} ({n})") };
        let candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

// ---------- save as file ----------

/// Writes `text` to `path`, which must be the file the user just picked in a save dialog (the
/// dialog plugin adds it to the asset scope); never a path in Co-pet's own folder. Error
/// `invalid` for any other path. The text is never logged.
pub fn write_text_file(app: &AppHandle, path: &str, text: &str) -> AppResult<()> {
    let path = Path::new(path);
    let picked = app.asset_protocol_scope().is_allowed(path);
    if !may_write(path, &app.state::<Paths>().root, picked) {
        return Err(AppError::Code("invalid"));
    }
    std::fs::write(path, text)?;
    Ok(())
}

/// An absolute file path, without `..`, outside Co-pet's folder (whose files the asset scope
/// allows anyway), that the save dialog `picked`.
fn may_write(path: &Path, app_root: &Path, picked: bool) -> bool {
    picked
        && path.is_absolute()
        && !path.components().any(|c| c == std::path::Component::ParentDir)
        && !path.starts_with(app_root)
        && !path.is_dir()
}

// ---------- delete everything ----------

/// Whether `typed`, after trimming, NFC, lowercasing and collapsing whitespace, is one of
/// `phrases` (each given in that form).
pub(crate) fn phrase_ok(typed: &str, phrases: &[&str]) -> bool {
    let typed: String = typed.trim().nfc().collect::<String>().to_lowercase();
    let typed = typed.split_whitespace().collect::<Vec<_>>().join(" ");
    phrases.contains(&typed.as_str())
}

/// Deletes the export tokens, the account session and sync key from the keychain, the database
/// and the library (models stay), then restarts into onboarding. Error `confirmation` when the
/// typed phrase doesn't match.
pub async fn delete_everything(app: &AppHandle, confirmation: &str) -> AppResult<()> {
    if !phrase_ok(confirmation, &CONFIRMATIONS) {
        return Err(AppError::Code("confirmation"));
    }
    app.state::<LlmServer>().stop().await;
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || -> AppResult<()> {
        // Read before the database goes.
        let user_id: Option<String> = settings::read(&app, "sync.userId")?;
        for account in ["notion", "asana", account::auth::SESSION] {
            secrets::delete(account)?;
        }
        if let Some(user_id) = user_id {
            secrets::delete(&account::keys::key_account(&user_id))?;
        }
        let paths = app.state::<Paths>().inner().clone();
        let db = app.state::<Db>();
        // Held until the process restarts: the triage worker and every command wait on it.
        let _hold = db.conn();
        for suffix in ["", "-wal", "-shm"] {
            let file = PathBuf::from(format!("{}{suffix}", paths.db.display()));
            match std::fs::remove_file(&file) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
        if paths.library.exists() {
            std::fs::remove_dir_all(&paths.library)?;
        }
        eprintln!("[data] deleted the keychain entries, the database and the library; restarting");
        // Exits through `RunEvent::Exit` (which stops `llama-server` again) and relaunches.
        app.restart();
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))?
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{may_write, phrase_ok, CONFIRMATIONS};
    use crate::account::CLOUD_PHRASES;

    #[test]
    fn writes_only_the_picked_file_outside_the_app_folder() {
        let root = Path::new("/Users/u/Library/Application Support/com.copet.app");
        let file = Path::new("/Users/u/Documents/Co-pet recovery key.txt");
        assert!(may_write(file, root, true));
        // Not picked in the save dialog.
        assert!(!may_write(file, root, false));
        // Co-pet's own files are in the asset scope already.
        assert!(!may_write(&root.join("library/x.txt"), root, true));
        assert!(!may_write(Path::new("relative.txt"), root, true));
        assert!(!may_write(Path::new("/Users/u/Documents/../Library/x.txt"), root, true));
        let dir = std::env::temp_dir();
        assert!(!may_write(&dir, root, true));
    }

    #[test]
    fn confirmation_accepts_both_languages_however_typed() {
        let ok = |typed: &str| phrase_ok(typed, &CONFIRMATIONS);
        assert!(ok("delete everything"));
        assert!(ok("  Delete   Everything \n"));
        assert!(ok("XÓA TẤT CẢ"));
        // Decomposed input (some Vietnamese input methods) and the older `xoá` spelling.
        assert!(ok("xo\u{301}a ta\u{302}\u{301}t ca\u{309}"));
        assert!(ok("xoá tất cả"));
        assert!(!ok("delete"));
        assert!(!ok("xoa tat ca"));
        assert!(!ok(""));
        assert!(!ok("delete cloud data"));

        let cloud = |typed: &str| phrase_ok(typed, &CLOUD_PHRASES);
        assert!(cloud("delete cloud data"));
        assert!(cloud(" Delete  Cloud\tData "));
        assert!(cloud("XÓA DỮ LIỆU ĐÁM MÂY"));
        assert!(cloud("xoá dữ liệu đám mây"));
        assert!(cloud("xo\u{301}a du\u{31b}\u{303} lie\u{302}\u{323}u \u{111}a\u{301}m ma\u{302}y"));
        assert!(!cloud("delete everything"));
        assert!(!cloud("xoa du lieu dam may"));
    }
}
