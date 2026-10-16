//! Library files. After a push, each pushed item's file and thumbnail that is on this Mac gets a
//! `sync_blobs` row: `up`, or `tooLarge` when the sealed file would exceed `sync.maxFileMb` MiB
//! (the bucket's per-object cap, so such files never leave this Mac). A pulled item's files that
//! aren't here get `down` (`apply`). Each run moves up to 20 due files: `up` seals the file with
//! the blob key and its library path as associated data and stores it as `blob_name(rel)`;
//! `down` fetches, opens and writes it atomically.
//!
//! A `down` file is never given up while its object is absent (the other Mac may not have
//! uploaded it yet, or it is over the limit there): it is tried again later, the wait growing
//! with the item's age (see [`absent_delay`]), with `updated_at` holding the next try. Only an
//! object that is there but won't open counts in `attempts`, and makes the file `missing` after
//! 3. Pulling the item again turns `missing` back into `down` (`apply`).
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
/// Runs that may find an object that won't open before the file is `missing`.
const MAX_ATTEMPTS: i64 = 3;
const MIB: u64 = 1_048_576;
const MIN_WAIT_MS: i64 = 60_000;
const MAX_WAIT_MS: i64 = 60 * 60_000;

/// The wait before asking again for an absent object of an item captured `age_ms` ago: the age
/// itself, between 1 min and 1 h. Each try then comes at about twice the age of the last, so
/// the waits double (1 min, 2 min, 4 min, …) up to one try an hour.
fn absent_delay(age_ms: i64) -> i64 {
    age_ms.clamp(MIN_WAIT_MS, MAX_WAIT_MS)
}

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

/// Moves up to 20 due files, oldest first. `Progress::Arrived` names the items whose file
/// arrived. A server error on one file is logged and that file goes to the back; going offline
/// or signed out ends the pass.
pub(crate) async fn run<R: Remote>(cx: &Ctx<'_>, remote: &R, on: &mut impl FnMut(Progress)) -> Result<(), SyncError> {
    let jobs: Vec<(String, String, i64)> = {
        let conn = cx.db.conn();
        let mut stmt = conn.prepare_cached(
            "SELECT rel, state, attempts FROM sync_blobs WHERE state IN ('up', 'down') AND updated_at <= ?1
             ORDER BY updated_at, rel LIMIT ?2",
        )?;
        let jobs = stmt
            .query_map(params![now_ms(), PER_RUN], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        jobs
    };
    if !jobs.is_empty() {
        on(Progress::Files);
    }
    for (rel, state, attempts) in jobs {
        let done = if state == "up" {
            upload(cx, remote, &rel).await.map(|()| None)
        } else {
            download(cx, remote, &rel, attempts).await
        };
        match done {
            Ok(Some(item_ids)) => on(Progress::Arrived(item_ids)),
            Ok(None) => {}
            Err(SyncError::Server(message)) => {
                eprintln!("[sync] file {rel}: {message}");
                cx.db.conn().execute("UPDATE sync_blobs SET updated_at = ?2 WHERE rel = ?1", params![rel, now_ms()])?;
            }
            Err(e) => return Err(e),
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
        let Some(sealed) = remote.get_blob(&blob_name(&cx.keys.name, rel)).await? else {
            let conn = cx.db.conn();
            let now = now_ms();
            let captured: Option<i64> = conn.query_row(
                "SELECT MIN(captured_at) FROM items WHERE file_path = ?1 OR thumb_path = ?1",
                [rel],
                |r| r.get(0),
            )?;
            let next = now + absent_delay(now - captured.unwrap_or(now));
            conn.execute("UPDATE sync_blobs SET updated_at = ?2 WHERE rel = ?1", params![rel, next])?;
            return Ok(None);
        };
        let (paths, key, owned) = (cx.paths.clone(), cx.keys.blob, rel.to_owned());
        let written = blocking(move || -> AppResult<bool> {
            let Ok(plain) = open(&key, owned.as_bytes(), &sealed) else { return Ok(false) };
            let staged = library::staging(&paths)?;
            std::fs::write(&staged, plain)?;
            library::commit(&paths, &staged, &owned)?;
            Ok(true)
        })
        .await??;
        if !written {
            eprintln!("[sync] file {rel} doesn't open");
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
    use crate::sync::remote::{MemoryRemote, RemoteRow};
    use crate::sync::testing::{device, file_item, keys, sync, sync_device, Device, USER};

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

        // The other Mac keeps the file `down` without counting attempts, and asks again later:
        // a minute for an item captured just now, an hour for an old one.
        sync(&b, &remote, &keys).await;
        let (_, next) = due(&b.db.conn(), &over_rel);
        assert_eq!(state(&b.db.conn(), &over_rel), ("down".into(), 0));
        assert!((next - now_ms() - MIN_WAIT_MS).abs() < 5_000, "next try in about a minute: {}", next - now_ms());
        sync(&b, &remote, &keys).await;
        assert_eq!(due(&b.db.conn(), &over_rel), ("down".into(), next), "not asked again before then");
        // An old item (written as apply would, so B doesn't queue it).
        b.db.conn()
            .execute_batch(&format!(
                "UPDATE sync_guard SET applying = 1; UPDATE items SET captured_at = {} WHERE id = '{over_id}';
                 UPDATE sync_guard SET applying = 0;",
                now_ms() - 3 * MAX_WAIT_MS
            ))
            .unwrap();
        for _ in 0..MAX_ATTEMPTS + 1 {
            make_due(&b, &over_rel);
            sync(&b, &remote, &keys).await;
        }
        let (state_now, next) = due(&b.db.conn(), &over_rel);
        assert_eq!(state_now, "down", "an absent object never makes the file missing");
        assert!((next - now_ms() - MAX_WAIT_MS).abs() < 5_000, "an old item waits an hour");
        assert!(b.paths.abs(&fits_rel).exists());
        assert!(!items::get_item_detail(&b.db.conn(), &over_id, &b.paths).unwrap().item.file_available);
    }

    fn due(conn: &Connection, rel: &str) -> (String, i64) {
        conn.query_row("SELECT state, updated_at FROM sync_blobs WHERE rel = ?1", [rel], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
    }

    /// As if the wait for `rel` were over.
    fn make_due(d: &Device, rel: &str) {
        d.db.conn().execute("UPDATE sync_blobs SET updated_at = 0 WHERE rel = ?1", [rel]).unwrap();
    }

    #[test]
    fn absent_waits_double_up_to_an_hour() {
        assert_eq!(absent_delay(0), MIN_WAIT_MS);
        assert_eq!(absent_delay(5 * MIN_WAIT_MS), 5 * MIN_WAIT_MS);
        assert_eq!(absent_delay(10 * MAX_WAIT_MS), MAX_WAIT_MS);
    }

    /// Refuses the upload of one object, as a server error.
    struct RefuseUpload<'a> {
        inner: &'a MemoryRemote,
        name: String,
    }

    impl Remote for RefuseUpload<'_> {
        async fn push(&self, rows: &[RemoteRow]) -> Result<u32, SyncError> {
            self.inner.push(rows).await
        }
        async fn pull(&self, after: i64, limit: usize) -> Result<Vec<RemoteRow>, SyncError> {
            self.inner.pull(after, limit).await
        }
        async fn put_blob(&self, name: &str, bytes: Vec<u8>) -> Result<(), SyncError> {
            if name == self.name {
                return Err(SyncError::Server("413: too big".into()));
            }
            self.inner.put_blob(name, bytes).await
        }
        async fn get_blob(&self, name: &str) -> Result<Option<Vec<u8>>, SyncError> {
            self.inner.get_blob(name).await
        }
    }

    #[tokio::test]
    async fn late_upload_still_arrives() {
        let (a, b, keys, remote) = (device("aaaa"), device("bbbb"), keys(), MemoryRemote::default());
        let (_, one) = file_item(&a, b"first file", "txt");
        let (_, two) = file_item(&a, b"second file", "txt");
        // The refused file is the one the pass reaches first.
        let (refused, other) = if one < two { (one, two) } else { (two, one) };
        let refuse = RefuseUpload { inner: &remote, name: blob_name(&keys.name, &refused) };
        sync_device(&a, &refuse, &keys).await.expect("a refused file doesn't fail the run");
        assert_eq!(state(&a.db.conn(), &other).0, "done", "the pass went on after the refusal");
        assert_eq!(state(&a.db.conn(), &refused).0, "up", "kept for the next run");

        // B pulls the item before its file is uploaded: it waits instead of giving up.
        sync(&b, &remote, &keys).await;
        assert_eq!(state(&b.db.conn(), &refused), ("down".into(), 0));
        assert!(b.paths.abs(&other).exists());
        sync(&a, &remote, &keys).await;
        assert_eq!(state(&a.db.conn(), &refused).0, "done");
        make_due(&b, &refused);
        sync(&b, &remote, &keys).await;
        assert_eq!(state(&b.db.conn(), &refused), ("done".into(), 0));
        assert_eq!(std::fs::read(b.paths.abs(&refused)).unwrap(), std::fs::read(a.paths.abs(&refused)).unwrap());
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
    async fn unopenable_blob_goes_missing_until_pulled_again() {
        let (a, b, keys, remote) = (device("aaaa"), device("bbbb"), keys(), MemoryRemote::default());
        let (id, rel) = file_item(&a, b"hello", "txt");
        sync(&a, &remote, &keys).await;
        // Same record and name keys, another blob key: the row applies, the file can't open.
        let other = Keys { blob: [9; 32], ..crate::sync::testing::keys() };
        for attempts in 1..=MAX_ATTEMPTS {
            sync(&b, &remote, &other).await;
            let expected = if attempts < MAX_ATTEMPTS { "down" } else { "missing" };
            assert_eq!(state(&b.db.conn(), &rel), (expected.into(), attempts));
        }
        sync(&b, &remote, &other).await;
        assert_eq!(state(&b.db.conn(), &rel), ("missing".into(), MAX_ATTEMPTS), "missing stays");
        assert!(!b.paths.abs(&rel).exists());

        // The item changes on A and is pulled again: the file is fetched afresh.
        items::set_pinned(&mut a.db.conn(), &id, true).unwrap();
        sync(&a, &remote, &keys).await;
        sync(&b, &remote, &keys).await;
        assert_eq!(state(&b.db.conn(), &rel), ("done".into(), 0));
        assert_eq!(std::fs::read(b.paths.abs(&rel)).unwrap(), b"hello");
    }
}
