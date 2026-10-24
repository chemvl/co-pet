//! Where the data folder and the models folder live. `locations.json` in the Tauri app data
//! folder (`Paths::base`) points to them; it sits outside the DB because the DB itself moves
//! and Delete everything wipes the DB but keeps the models. The models move live; the data
//! moves at the next start, before the DB opens. Unmoved, both sit in [`default_root`].
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::ipc::CapabilityBuilder;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

use crate::error::{AppError, AppResult};
use crate::events;
use crate::i18n::{system_lang, tr};
use crate::llm::download::Downloader;
use crate::llm::models::REGISTRY;
use crate::llm::LlmServer;
use crate::model::StorageLocations;
use crate::paths::Paths;
use crate::transcribe::models::{PackDownloader, SPEAKER, SPEECH};
use crate::transcribe::worker::TranscribeQueue;

pub const LOCATIONS_FILE: &str = "locations.json";
pub const DATA_FOLDER: &str = "Co-pet data";
pub const MODELS_FOLDER: &str = "Co-pet models";
/// The MSIX package's default data folder, in the home folder.
#[cfg(windows)]
const PACKAGED_ROOT: &str = "Co-pet";
/// Data entries that move; tmp/ is recreated empty at startup.
const DATA_ENTRIES: [&str; 5] = ["copet.db", "copet.db-wal", "copet.db-shm", "library", "recordings"];
const WRITE_TEST: &str = ".copet-write-test";

/// `locations.json`. `None` folders are the defaults: `default_root` for data, `default_root/models`
/// for models.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Locations {
    pub data_dir: Option<PathBuf>,
    pub models_dir: Option<PathBuf>,
    /// The data folder moves here at the next start.
    pub pending_data_dir: Option<PathBuf>,
    /// The error code of the last failed startup move.
    pub move_error: Option<String>,
}

/// The default data folder: `base`, or `%USERPROFILE%\Co-pet` in the MSIX package, whose writes
/// under `%APPDATA%` are virtualized into the package and so hidden from agents and Explorer.
#[cfg_attr(not(windows), allow(unused_variables))]
pub fn default_root(app: &AppHandle, base: &Path) -> AppResult<PathBuf> {
    #[cfg(windows)]
    if crate::platform::win::package::is_packaged() {
        return Ok(app.path().home_dir()?.join(PACKAGED_ROOT));
    }
    Ok(base.to_path_buf())
}

/// Reads `locations.json`; missing or unreadable gives the defaults.
pub fn load(base: &Path) -> Locations {
    let path = base.join(LOCATIONS_FILE);
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            eprintln!("[storage] {} is unreadable, using the defaults: {e}", path.display());
            Locations::default()
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Locations::default(),
        Err(e) => {
            eprintln!("[storage] reading {}: {e}", path.display());
            Locations::default()
        }
    }
}

/// Writes `locations.json` atomically (a temp file renamed over it).
pub fn save(base: &Path, l: &Locations) -> AppResult<()> {
    std::fs::create_dir_all(base)?;
    let tmp = base.join(format!("{LOCATIONS_FILE}.tmp"));
    std::fs::write(&tmp, serde_json::to_vec_pretty(l)?)?;
    std::fs::rename(&tmp, base.join(LOCATIONS_FILE))?;
    Ok(())
}

/// The folder Co-pet uses inside the picked one; a picked folder already named `name` is used
/// as is, so picking it again doesn't nest.
pub fn target_dir(picked: &Path, name: &str) -> PathBuf {
    if picked.file_name().is_some_and(|n| n == name) {
        picked.to_path_buf()
    } else {
        picked.join(name)
    }
}

/// Creates `dir` and writes a probe file there. Error `folderNotWritable`.
pub fn check_writable(dir: &Path) -> AppResult<()> {
    let probe = dir.join(WRITE_TEST);
    std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(&probe, b""))
        .and_then(|()| std::fs::remove_file(&probe))
        .map_err(|e| {
            eprintln!("[storage] {} is not writable: {e}", dir.display());
            AppError::Code("folderNotWritable")
        })
}

/// Error `notEnoughSpace` when `to` is on another disk with less than `bytes` free. Same disk
/// (a rename needs no space) or an unknown disk passes.
fn needs_space(from: &Path, to: &Path, bytes: u64) -> AppResult<()> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let disk_of = |p: &Path| {
        disks
            .list()
            .iter()
            .filter(|d| p.starts_with(d.mount_point()))
            .max_by_key(|d| d.mount_point().as_os_str().len())
    };
    match (disk_of(from), disk_of(to)) {
        (Some(a), Some(b)) if a.mount_point() == b.mount_point() => Ok(()),
        (_, Some(b)) if b.available_space() < bytes => Err(AppError::Code("notEnoughSpace")),
        _ => Ok(()),
    }
}

/// A file's length, or the recursive sum for a folder; missing → 0.
fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path)
        .map(|entries| entries.flatten().map(|e| size_of(&e.path())).sum())
        .unwrap_or(0)
}

/// Any `DATA_ENTRIES` in `dir`.
fn holds_data(dir: &Path) -> bool {
    DATA_ENTRIES.iter().any(|n| std::fs::symlink_metadata(dir.join(n)).is_ok())
}

/// Tells the user the data folder is gone, then quits when they dismiss it.
pub fn show_missing_and_quit(app: &AppHandle, missing: &Path) {
    let lang = system_lang();
    let h = app.clone();
    app.dialog()
        .message(tr(lang, "storage.missing").replace("{path}", &missing.to_string_lossy()))
        .title(tr(lang, "storage.missingTitle"))
        .kind(MessageDialogKind::Error)
        .show(move |_| h.exit(1));
}

fn copy_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(src)?.is_dir() {
        std::fs::create_dir(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_all(&entry.path(), &dst.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(src, dst).map(|_| ())
    }
}

fn remove_all(path: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(path)?.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

#[derive(Clone, Copy)]
enum Moved {
    /// Renamed to the target: the source no longer exists.
    Renamed,
    /// Copied to the target: the source stays until the commit succeeds.
    Copied,
    /// Already at the target with the same length: left there; the source stays until commit.
    Duplicate,
}

/// Puts every moved entry back: renamed entries are renamed back, copied entries are removed
/// from `to` (their sources were never touched), duplicates stay as they were.
fn undo(from: &Path, to: &Path, done: &[(&str, Moved)], rename: fn(&Path, &Path) -> std::io::Result<()>) {
    for &(name, moved) in done.iter().rev() {
        let (src, dst) = (from.join(name), to.join(name));
        match moved {
            Moved::Renamed => {
                if let Err(e) = rename(&dst, &src) {
                    // The entry exists only at `dst`: copy it back before removing it.
                    match copy_all(&dst, &src) {
                        Ok(()) => {
                            if let Err(e) = remove_all(&dst) {
                                eprintln!("[storage] undo: removing {}: {e}", dst.display());
                            }
                        }
                        Err(e2) => eprintln!(
                            "[storage] undo: {} stays at {} (rename back: {e}; copy back: {e2})",
                            name,
                            dst.display()
                        ),
                    }
                }
            }
            Moved::Copied => {
                if let Err(e) = remove_all(&dst) {
                    eprintln!("[storage] undo: removing the copy {}: {e}", dst.display());
                }
            }
            Moved::Duplicate => {}
        }
    }
}

/// Moves the `names` entries (missing ones are skipped) from `from` to `to`, then runs
/// `commit`. Each entry is renamed, or copied when the rename fails (another drive). With
/// `same_len_skip`, a file already at the target with the source's length is kept there;
/// otherwise an existing target entry is replaced (`same_len_skip`, models only) or refused
/// (`folderInUse`). Any failure, the commit's included, undoes every entry. Sources of copied
/// and duplicate entries are removed only after the commit succeeds. Errors: the commit's
/// code, `folderInUse`, else `moveFailed`.
fn relocate_with(
    from: &Path,
    to: &Path,
    names: &[String],
    same_len_skip: bool,
    rename: fn(&Path, &Path) -> std::io::Result<()>,
    commit: impl FnOnce() -> AppResult<()>,
) -> AppResult<()> {
    let mut done: Vec<(&str, Moved)> = Vec::new();
    let mut move_all = || -> AppResult<()> {
        std::fs::create_dir_all(to)?;
        for name in names {
            let (src, dst) = (from.join(name), to.join(name));
            let src_meta = match std::fs::symlink_metadata(&src) {
                Ok(m) => m,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            if let Ok(dst_meta) = std::fs::symlink_metadata(&dst) {
                if !same_len_skip {
                    return Err(AppError::Code("folderInUse"));
                }
                if src_meta.is_file() && dst_meta.is_file() && src_meta.len() == dst_meta.len() {
                    done.push((name, Moved::Duplicate));
                    continue;
                }
                remove_all(&dst)?;
            }
            if rename(&src, &dst).is_ok() {
                done.push((name, Moved::Renamed));
                continue;
            }
            if let Err(e) = copy_all(&src, &dst) {
                // A partial copy: `dst` did not exist before, so removing it loses nothing.
                if let Err(e) = remove_all(&dst) {
                    eprintln!("[storage] removing the partial copy {}: {e}", dst.display());
                }
                return Err(e.into());
            }
            done.push((name, Moved::Copied));
        }
        Ok(())
    };
    if let Err(e) = move_all().and_then(|()| commit()) {
        undo(from, to, &done, rename);
        return Err(match e {
            AppError::Code(c) => AppError::Code(c),
            other => {
                eprintln!("[storage] moving {} to {}: {other}", from.display(), to.display());
                AppError::Code("moveFailed")
            }
        });
    }
    for &(name, moved) in &done {
        if matches!(moved, Moved::Copied | Moved::Duplicate) {
            let src = from.join(name);
            if let Err(e) = remove_all(&src) {
                eprintln!("[storage] removing the moved source {}: {e}", src.display());
            }
        }
    }
    Ok(())
}

fn relocate(from: &Path, to: &Path, names: &[String], same_len_skip: bool, commit: impl FnOnce() -> AppResult<()>) -> AppResult<()> {
    relocate_with(from, to, names, same_len_skip, |a, b| std::fs::rename(a, b), commit)
}

/// Waits until the restarting process has let go of `copet.db` (up to 10 s).
#[cfg(windows)]
fn wait_for_db(db: &Path) {
    use std::os::windows::fs::OpenOptionsExt;
    if !db.exists() {
        return;
    }
    for _ in 0..40 {
        if std::fs::OpenOptions::new().read(true).share_mode(0).open(db).is_ok() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

#[cfg(not(windows))]
fn wait_for_db(_db: &Path) {}

/// Resolves the locations at startup, running a pending data move first. A failed move keeps
/// the data where it was and records `moveError`. Error `dataFolderMissing` when the data
/// folder is gone (an unplugged drive); a pending move then waits for it too. `default_root` is
/// the data folder while `locations.json` names none.
pub fn startup(base: &Path, default_root: &Path) -> AppResult<Locations> {
    let mut loc = load(base);
    if loc.data_dir.as_deref().is_some_and(|d| !d.exists()) {
        return Err(AppError::Code("dataFolderMissing"));
    }
    let Some(target) = loc.pending_data_dir.clone() else { return Ok(loc) };
    let from = loc.data_dir.clone().unwrap_or_else(|| default_root.to_path_buf());
    let mut next = loc.clone();
    next.data_dir = (target != default_root).then(|| target.clone());
    next.pending_data_dir = None;
    next.move_error = None;
    let result = if target == from {
        save(base, &next)
    } else if holds_data(&target) {
        Err(AppError::Code("folderInUse"))
    } else {
        wait_for_db(&from.join("copet.db"));
        let names = DATA_ENTRIES.map(String::from);
        relocate(&from, &target, &names, false, || save(base, &next))
    };
    match result {
        Ok(()) => Ok(next),
        Err(e) => {
            let code = match e {
                AppError::Code(c) => c.to_string(),
                other => {
                    eprintln!("[storage] data move to {}: {other}", target.display());
                    "moveFailed".to_string()
                }
            };
            eprintln!("[storage] data move to {} failed: {code}", target.display());
            loc.pending_data_dir = None;
            loc.move_error = Some(code);
            if let Err(e) = save(base, &loc) {
                eprintln!("[storage] saving {LOCATIONS_FILE}: {e}");
            }
            Ok(loc)
        }
    }
}

/// `glob::Pattern` escapes, so a folder name with `[`, `]`, `*` or `?` matches literally.
fn glob_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '[' => out.push_str("[[]"),
            ']' => out.push_str("[]]"),
            '*' => out.push_str("[*]"),
            '?' => out.push_str("[?]"),
            c => out.push(c),
        }
    }
    out
}

/// A moved data folder, or the MSIX package's default one, is outside the `$APPDATA` scopes in
/// `tauri.conf.json` and the default capability: grants the asset protocol and the opener for it
/// at runtime.
pub fn grant_scopes(app: &AppHandle, paths: &Paths) -> AppResult<()> {
    if paths.root == paths.base {
        return Ok(());
    }
    let scope = app.asset_protocol_scope();
    for d in [&paths.library, &paths.tmp, &paths.recordings] {
        scope.allow_directory(d, true)?;
    }
    let pattern = format!("{}{}**", glob_escape(&paths.library.to_string_lossy()), std::path::MAIN_SEPARATOR);
    app.add_capability(
        CapabilityBuilder::new("data-folder")
            .windows(["pet", "home", "onboarding"])
            .permission_scoped("opener:allow-open-path", vec![json!({ "path": pattern })], Vec::<Value>::new()),
    )?;
    Ok(())
}

pub fn view(paths: &Paths) -> StorageLocations {
    let loc = load(&paths.base);
    let models = paths.models();
    StorageLocations {
        data_dir: paths.root.to_string_lossy().into_owned(),
        data_default: paths.root == paths.default_root,
        pending_data_dir: loc.pending_data_dir.map(|p| p.to_string_lossy().into_owned()),
        models_default: models == paths.default_root.join("models"),
        models_dir: models.to_string_lossy().into_owned(),
        move_error: loc.move_error,
    }
}

/// `name` inside the picked folder. Error `invalid` for a relative path.
fn picked_dir(dir: &str, name: &str) -> AppResult<PathBuf> {
    let picked = Path::new(dir);
    if !picked.is_absolute() {
        return Err(AppError::Code("invalid"));
    }
    Ok(target_dir(picked, name))
}

/// Moves the local models to `dir` (`None`: the default) while downloads, transcription and
/// llama-server are paused. Errors: `invalid`, `folderNotWritable`, `transcribing`,
/// `notEnoughSpace`, `moveFailed`.
pub async fn set_models_dir(app: &AppHandle, dir: Option<String>) -> AppResult<StorageLocations> {
    let paths = app.state::<Paths>().inner().clone();
    let default = paths.default_root.join("models");
    let target = match dir {
        None => default.clone(),
        Some(d) => picked_dir(&d, MODELS_FOLDER)?,
    };
    let old = paths.models();
    if target == old {
        return Ok(view(&paths));
    }
    check_writable(&target)?;
    let queue = app.state::<TranscribeQueue>();
    if !queue.hold_for_models_move() {
        return Err(AppError::Code("transcribing"));
    }
    let downloader = app.state::<Downloader>();
    let pack_downloader = app.state::<PackDownloader>();
    let ai = downloader.pause_for_move(app).await;
    let packs = pack_downloader.pause_for_move(app).await;
    app.state::<LlmServer>().stop().await;

    let moving = paths.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let names: Vec<String> = REGISTRY
            .iter()
            .chain(&SPEECH)
            .chain(&SPEAKER)
            .flat_map(|spec| [spec.file.to_string(), format!("{}.part", spec.file)])
            .collect();
        needs_space(&old, &target, names.iter().map(|n| size_of(&old.join(n))).sum())?;
        relocate(&old, &target, &names, true, || {
            let mut loc = load(&moving.base);
            loc.models_dir = (target != default).then(|| target.clone());
            save(&moving.base, &loc)?;
            moving.set_models(target.clone());
            Ok(())
        })
    })
    .await
    .map_err(|e| AppError::msg(e.to_string()))
    .and_then(|r| r);

    if let Some(id) = ai {
        if let Err(e) = downloader.start(app, id) {
            eprintln!("[storage] resuming the {id} download: {e}");
        }
    }
    for p in packs {
        if let Err(e) = pack_downloader.start(app, p) {
            eprintln!("[storage] resuming the {} pack download: {e}", p.as_str());
        }
    }
    queue.end_models_move();
    events::ai_status(app, downloader.status(app));
    events::speech_status(app, pack_downloader.status(app));
    result.map(|()| view(&paths))
}

/// Schedules the data folder's move to `dir` (`None`: the default) for the next start; the
/// current folder cancels a pending move. Errors: `invalid` (relative or nested),
/// `folderInUse`, `folderNotWritable`, `notEnoughSpace`.
pub fn set_data_dir(paths: &Paths, dir: Option<String>) -> AppResult<StorageLocations> {
    let target = match dir {
        None => paths.default_root.clone(),
        Some(d) => picked_dir(&d, DATA_FOLDER)?,
    };
    let mut loc = load(&paths.base);
    if target == paths.root {
        loc.pending_data_dir = None;
    } else {
        if target != paths.default_root && (target.starts_with(&paths.root) || paths.root.starts_with(&target)) {
            return Err(AppError::Code("invalid"));
        }
        if holds_data(&target) {
            return Err(AppError::Code("folderInUse"));
        }
        check_writable(&target)?;
        needs_space(&paths.root, &target, DATA_ENTRIES.iter().map(|n| size_of(&paths.root.join(n))).sum())?;
        loc.pending_data_dir = Some(target);
    }
    loc.move_error = None;
    save(&paths.base, &loc)?;
    Ok(view(paths))
}

pub fn cancel_data_move(paths: &Paths) -> AppResult<StorageLocations> {
    let mut loc = load(&paths.base);
    loc.pending_data_dir = None;
    save(&paths.base, &loc)?;
    Ok(view(paths))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fail_rename(_: &Path, _: &Path) -> std::io::Result<()> {
        Err(std::io::Error::other("cross-device"))
    }

    fn std_rename(a: &Path, b: &Path) -> std::io::Result<()> {
        std::fs::rename(a, b)
    }

    fn data_names() -> Vec<String> {
        DATA_ENTRIES.map(String::from).to_vec()
    }

    /// `copet.db` and `library/ab/x.pdf` under `dir`.
    fn seed(dir: &Path) {
        std::fs::create_dir_all(dir.join("library").join("ab")).unwrap();
        std::fs::write(dir.join("copet.db"), b"db").unwrap();
        std::fs::write(dir.join("library").join("ab").join("x.pdf"), b"pdf").unwrap();
    }

    fn seeded(dir: &Path) -> bool {
        std::fs::read(dir.join("copet.db")).is_ok_and(|b| b == b"db")
            && std::fs::read(dir.join("library").join("ab").join("x.pdf")).is_ok_and(|b| b == b"pdf")
    }

    fn empty(dir: &Path) -> bool {
        !dir.join("copet.db").exists() && !dir.join("library").exists()
    }

    #[test]
    fn target_dir_appends_the_folder_once() {
        let named = Path::new(r"D:\Stuff\Co-pet data");
        assert_eq!(target_dir(named, DATA_FOLDER), named);
        assert_eq!(target_dir(Path::new(r"D:\"), DATA_FOLDER), Path::new(r"D:\").join(DATA_FOLDER));
    }

    #[test]
    fn relocate_moves_by_rename_and_by_copy() {
        for rename in [std_rename as fn(&Path, &Path) -> std::io::Result<()>, fail_rename] {
            let dir = tempfile::tempdir().unwrap();
            let (from, to) = (dir.path().join("from"), dir.path().join("to"));
            seed(&from);
            relocate_with(&from, &to, &data_names(), false, rename, || Ok(())).unwrap();
            assert!(seeded(&to));
            assert!(empty(&from), "sources are gone");
        }
    }

    #[test]
    fn failed_commit_leaves_everything_at_the_source() {
        for rename in [std_rename as fn(&Path, &Path) -> std::io::Result<()>, fail_rename] {
            let dir = tempfile::tempdir().unwrap();
            let (from, to) = (dir.path().join("from"), dir.path().join("to"));
            seed(&from);
            let r = relocate_with(&from, &to, &data_names(), false, rename, || Err(AppError::msg("disk full")));
            assert!(matches!(r, Err(AppError::Code("moveFailed"))), "{r:?}");
            assert!(seeded(&from));
            assert!(empty(&to));
        }
    }

    #[test]
    fn same_len_skip_keeps_the_target_file() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("from"), dir.path().join("to"));
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(from.join("m.gguf"), b"abc").unwrap();
        std::fs::write(to.join("m.gguf"), b"xyz").unwrap();
        relocate_with(&from, &to, &["m.gguf".to_string()], true, std_rename, || Ok(())).unwrap();
        assert_eq!(std::fs::read(to.join("m.gguf")).unwrap(), b"xyz");
        assert!(!from.join("m.gguf").exists());
    }

    #[test]
    fn startup_moves_pending_data() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("base");
        let target = dir.path().join("D").join(DATA_FOLDER);
        seed(&base);
        save(&base, &Locations { pending_data_dir: Some(target.clone()), ..Default::default() }).unwrap();
        let loc = startup(&base, &base).unwrap();
        assert_eq!(loc, Locations { data_dir: Some(target.clone()), ..Default::default() });
        assert_eq!(load(&base), loc, "persisted");
        assert!(seeded(&target));
        assert!(empty(&base));
    }

    #[test]
    fn startup_moves_back_to_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("base");
        let data = dir.path().join("D").join(DATA_FOLDER);
        seed(&data);
        let l = Locations { data_dir: Some(data.clone()), pending_data_dir: Some(base.clone()), ..Default::default() };
        save(&base, &l).unwrap();
        let loc = startup(&base, &base).unwrap();
        assert_eq!(loc.data_dir, None);
        assert_eq!(loc.pending_data_dir, None);
        assert!(seeded(&base));
        assert!(empty(&data));
    }

    #[test]
    fn startup_moves_back_to_a_default_outside_base() {
        // The MSIX package: `locations.json` in the app data folder, the data in the home folder.
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("base");
        let home = dir.path().join("home").join("Co-pet");
        let data = dir.path().join("D").join(DATA_FOLDER);
        seed(&data);
        let l = Locations { data_dir: Some(data.clone()), pending_data_dir: Some(home.clone()), ..Default::default() };
        save(&base, &l).unwrap();
        let loc = startup(&base, &home).unwrap();
        assert_eq!(loc, Locations::default());
        assert_eq!(load(&base), loc, "persisted in base");
        assert!(seeded(&home));
        assert!(empty(&data));
        assert!(empty(&base));
    }

    #[test]
    fn startup_refuses_a_folder_with_data() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("base");
        let target = dir.path().join("D").join(DATA_FOLDER);
        seed(&base);
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("copet.db"), b"other").unwrap();
        save(&base, &Locations { pending_data_dir: Some(target.clone()), ..Default::default() }).unwrap();
        let loc = startup(&base, &base).unwrap();
        assert_eq!(loc.move_error.as_deref(), Some("folderInUse"));
        assert_eq!((loc.data_dir, loc.pending_data_dir), (None, None));
        assert_eq!(load(&base).move_error.as_deref(), Some("folderInUse"));
        assert!(seeded(&base));
        assert_eq!(std::fs::read(target.join("copet.db")).unwrap(), b"other");
    }

    #[test]
    fn startup_reports_a_missing_data_folder() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("base");
        let gone = dir.path().join("D").join(DATA_FOLDER);
        save(&base, &Locations { data_dir: Some(gone), ..Default::default() }).unwrap();
        assert!(matches!(startup(&base, &base), Err(AppError::Code("dataFolderMissing"))));
    }

    #[test]
    fn unparsable_locations_give_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(LOCATIONS_FILE), b"{not json").unwrap();
        assert_eq!(load(dir.path()), Locations::default());
    }
}
