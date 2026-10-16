//! Library files. After a push, each pushed item's file and thumbnail that is on this Mac gets a
//! `sync_blobs` row: `up`, or `tooLarge` when the sealed file would exceed `sync.maxFileMb` MiB
//! (the bucket's per-object cap, so such files never leave this Mac). A pulled item's files that
//! aren't here get `down` (`apply`). Each run moves up to 20 files: `up` seals the file with the
//! blob key and its library path as associated data and stores it as `blob_name(rel)`; `down`
//! fetches, opens and writes it atomically, and after 3 runs without a usable object marks it
//! `missing`.
use std::io::ErrorKind;
use std::path::{Component, Path};

use rusqlite::{params, Connection, OptionalExtension};

use super::codec::blob_name;
use super::engine::{Ctx, Progress};
use super::remote::{Remote, SyncError};
use crate::account::crypto::{open, seal, SEAL_OVERHEAD};
use crate::db::{now_ms, settings};
use crate::error::{AppError, AppResult};
use crate::library;
use crate::paths::Paths;

/// Files moved per run.
const PER_RUN: i64 = 20;
/// Runs a download may find no usable object before the file is `missing`.
const MAX_ATTEMPTS: i64 = 3;
const MIB: u64 = 1_048_576;

/// Whether a file of `size` bytes, once sealed, is over `max_mb` MiB.
fn too_large(size: u64, max_mb: u64) -> bool {
    size + SEAL_OVERHEAD as u64 > max_mb * MIB
}

/// After `item_ids` were pushed: each of their files that is on this Mac and has no
/// `sync_blobs` row gets one, `up` or `tooLarge`.
pub(crate) fn register(conn: &Connection, paths: &Paths, item_ids: &[String], now: i64) -> AppResult<()> {
    if item_ids.is_empty() {
        return Ok(());
    }
    let max_mb: u64 = settings::get(conn, "sync.maxFileMb")?;
    let mut files = conn.prepare_cached("SELECT file_path, thumb_path FROM items WHERE id = ?1")?;
    let mut insert = conn.prepare_cached("INSERT OR IGNORE INTO sync_blobs (rel, state, updated_at) VALUES (?1, ?2, ?3)")?;
    for id in item_ids {
        let found = files
            .query_row([id], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?)))
            .optional()?;
        let Some((file, thumb)) = found else { continue };
        for rel in [file, thumb].into_iter().flatten() {
            let Ok(meta) = std::fs::metadata(paths.abs(&rel)) else { continue };
            let state = if too_large(meta.len(), max_mb) { "tooLarge" } else { "up" };
            insert.execute(params![rel, state, now])?;
        }
    }
    Ok(())
}

/// Moves up to 20 files, oldest first. `Progress::Arrived` names the items whose file arrived.
pub(crate) async fn run<R: Remote>(cx: &Ctx<'_>, remote: &R, on: &mut impl FnMut(Progress)) -> Result<(), SyncError> {
    let jobs: Vec<(String, String, i64)> = {
        let conn = cx.db.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT rel, state, attempts FROM sync_blobs WHERE state IN ('up', 'down') ORDER BY updated_at, rel LIMIT ?1",
        )?;
        let jobs = stmt.query_map([PER_RUN], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
        jobs
    };
    for (rel, state, attempts) in jobs {
        if state == "up" {
            upload(cx, remote, &rel).await?;
        } else if let Some(item_ids) = download(cx, remote, &rel, attempts).await? {
            on(Progress::Arrived(item_ids));
        }
    }
    Ok(())
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, SyncError> {
    tokio::task::spawn_blocking(f).await.map_err(|e| SyncError::Server(e.to_string()))
}

fn set_state(conn: &Connection, rel: &str, state: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE sync_blobs SET state = ?2, updated_at = ?3 WHERE rel = ?1",
        params![rel, state, now_ms()],
    )?;
    Ok(())
}

async fn upload<R: Remote>(cx: &Ctx<'_>, remote: &R, rel: &str) -> Result<(), SyncError> {
    let (path, key, aad) = (cx.paths.abs(rel), cx.keys.blob, rel.to_owned());
    let sealed = blocking(move || std::fs::read(path).map(|plain| seal(&key, aad.as_bytes(), &plain))).await?;
    let state = match sealed {
        Ok(bytes) => {
            remote.put_blob(&blob_name(&cx.keys.name, rel), bytes).await?;
            "done"
        }
        // Removed from the library since it was queued.
        Err(e) if e.kind() == ErrorKind::NotFound => "missing",
        Err(e) => return Err(AppError::from(e).into()),
    };
    set_state(&cx.db.conn(), rel, state)?;
    Ok(())
}

/// A library-relative path that stays inside the library (`ab/<sha>.pdf`, `thumbs/<sha>.png`).
fn inside_library(rel: &str) -> bool {
    let path = Path::new(rel);
    path.components().next().is_some() && path.components().all(|c| matches!(c, Component::Normal(_)))
}

/// The ids of the items using `rel` once it is here, `None` while it isn't.
async fn download<R: Remote>(cx: &Ctx<'_>, remote: &R, rel: &str, attempts: i64) -> Result<Option<Vec<String>>, SyncError> {
    if !inside_library(rel) {
        eprintln!("[sync] not a library path: {rel}");
        set_state(&cx.db.conn(), rel, "missing")?;
        return Ok(None);
    }
    if !cx.paths.abs(rel).exists() {
        let written = match remote.get_blob(&blob_name(&cx.keys.name, rel)).await? {
            Some(sealed) => {
                let (paths, key, rel) = (cx.paths.clone(), cx.keys.blob, rel.to_owned());
                blocking(move || -> AppResult<bool> {
                    let Ok(plain) = open(&key, rel.as_bytes(), &sealed) else { return Ok(false) };
                    let staged = library::staging(&paths)?;
                    std::fs::write(&staged, plain)?;
                    library::commit(&paths, &staged, &rel)?;
                    Ok(true)
                })
                .await??
            }
            None => false,
        };
        if !written {
            let attempts = attempts + 1;
            let state = if attempts >= MAX_ATTEMPTS { "missing" } else { "down" };
            cx.db.conn().execute(
                "UPDATE sync_blobs SET state = ?2, attempts = ?3, updated_at = ?4 WHERE rel = ?1",
                params![rel, state, attempts, now_ms()],
            )?;
            return Ok(None);
        }
    }
    let conn = cx.db.conn();
    set_state(&conn, rel, "done")?;
    let mut stmt = conn.prepare_cached("SELECT id FROM items WHERE file_path = ?1 OR thumb_path = ?1")?;
    let ids = stmt.query_map([rel], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    Ok(Some(ids))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::items;
    use crate::sync::codec::Keys;
    use crate::sync::engine::sync_with;
    use crate::sync::remote::MemoryRemote;
    use crate::sync::testing::{device, file_item, keys, sync, USER};

    fn state(conn: &Connection, rel: &str) -> (String, i64) {
        conn.query_row("SELECT state, attempts FROM sync_blobs WHERE rel = ?1", [rel], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[tokio::test]
    async fn blob_round_trip() {
        let (a, b, keys, remote) = (device("aaaa"), device("bbbb"), keys(), MemoryRemote::default());
        let bytes = b"%PDF-1.7 quarterly numbers ".repeat(64);
        let (id, rel) = file_item(&a, &bytes, "pdf");
        sync(&a, &remote, &keys).await;
        assert_eq!(state(&a.db.conn(), &rel), ("done".into(), 0));
        let blobs = remote.blobs();
        assert_eq!(blobs.len(), 1);
        let sealed = &blobs[&blob_name(&keys.name, &rel)];
        assert_eq!(sealed.len(), bytes.len() + SEAL_OVERHEAD);
        assert!(!contains(sealed, b"quarterly"), "the server stores ciphertext");
        assert!(blobs.keys().all(|name| !name.contains(&rel) && name.len() == 64), "object names are HMACs");

        let mut arrived = Vec::new();
        let cx = Ctx { db: &b.db, paths: &b.paths, keys: &keys, user_id: USER };
        sync_with(&cx, &remote, &mut |p| {
            if let Progress::Arrived(ids) = p {
                arrived.extend(ids);
            }
        })
        .await
        .unwrap();
        assert_eq!(arrived, [id.clone()]);
        assert_eq!(std::fs::read(b.paths.abs(&rel)).unwrap(), bytes);
        assert_eq!(state(&b.db.conn(), &rel), ("done".into(), 0));
        assert!(items::get_item_detail(&b.db.conn(), &id, &b.paths).unwrap().item.file_available);
        std::fs::remove_file(b.paths.abs(&rel)).unwrap();
        assert!(!items::get_item_detail(&b.db.conn(), &id, &b.paths).unwrap().item.file_available);
    }

    #[tokio::test]
    async fn too_large_blob_not_uploaded() {
        let (a, b, keys, remote) = (device("aaaa"), device("bbbb"), keys(), MemoryRemote::default());
        settings::put(&a.db.conn(), "sync.maxFileMb", &serde_json::json!(1)).unwrap();
        // Sealed, the first is exactly 1 MiB (fits); the second is one byte over.
        let fits = vec![1u8; MIB as usize - SEAL_OVERHEAD];
        let over = vec![2u8; MIB as usize - SEAL_OVERHEAD + 1];
        let (_, fits_rel) = file_item(&a, &fits, "bin");
        let (over_id, over_rel) = file_item(&a, &over, "mov");
        sync(&a, &remote, &keys).await;
        assert_eq!(state(&a.db.conn(), &fits_rel).0, "done");
        assert_eq!(state(&a.db.conn(), &over_rel).0, "tooLarge");
        let blobs = remote.blobs();
        assert_eq!(blobs.len(), 1);
        assert_eq!(blobs[&blob_name(&keys.name, &fits_rel)].len() as u64, MIB);

        // The other Mac tries once per run, then gives up; the item says its file is elsewhere.
        for attempts in 1..=MAX_ATTEMPTS {
            sync(&b, &remote, &keys).await;
            let expected = if attempts < MAX_ATTEMPTS { "down" } else { "missing" };
            assert_eq!(state(&b.db.conn(), &over_rel), (expected.into(), attempts));
        }
        sync(&b, &remote, &keys).await;
        assert_eq!(state(&b.db.conn(), &over_rel), ("missing".into(), MAX_ATTEMPTS), "missing is final");
        assert!(b.paths.abs(&fits_rel).exists());
        assert!(!items::get_item_detail(&b.db.conn(), &over_id, &b.paths).unwrap().item.file_available);
    }

    #[test]
    fn size_limit_counts_the_seal() {
        assert!(!too_large(50 * MIB - SEAL_OVERHEAD as u64, 50));
        assert!(too_large(50 * MIB - SEAL_OVERHEAD as u64 + 1, 50));
        assert!(too_large(50 * MIB, 50), "a file of exactly the limit doesn't fit once sealed");
    }

    #[test]
    fn library_paths_only() {
        assert!(inside_library("ab/abcd.pdf") && inside_library("thumbs/abcd.png"));
        for bad in ["", "/etc/hosts", "../x", "ab/../../x", "./ab/x"] {
            assert!(!inside_library(bad), "{bad}");
        }
    }

    #[tokio::test]
    async fn wrong_blob_key_counts_as_missing() {
        let (a, b, keys, remote) = (device("aaaa"), device("bbbb"), keys(), MemoryRemote::default());
        let (_, rel) = file_item(&a, b"hello", "txt");
        sync(&a, &remote, &keys).await;
        // Same record and name keys, another blob key: the row applies, the file can't open.
        let other = Keys { blob: [9; 32], ..crate::sync::testing::keys() };
        sync(&b, &remote, &other).await;
        assert_eq!(state(&b.db.conn(), &rel), ("down".into(), 1));
        assert!(!b.paths.abs(&rel).exists());
    }
}
