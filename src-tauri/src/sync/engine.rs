//! The sync loop and one run. A run pushes `sync_outbox` (batches of 100, parents first), pulls
//! by server `seq` from `sync_state.cursor` (pages of 100, each applied in one transaction with
//! its cursor) and moves up to 20 files. The loop runs after a `poke` (2 s later, so a burst of
//! changes is one run) or every 60 s while sync is on; while it is off, paused or waiting for a
//! key or sign-in, only a poke wakes it, and that run does no network or database work.
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use rusqlite::{params, Connection};
use serde_json::Value;
use tauri::{AppHandle, Manager};
use tokio::sync::Notify;

use super::apply::{apply_page, Applied};
use super::blobs;
use super::codec::{read_row, seal_row, Keys};
use super::remote::{Remote, RemoteRow, SupabaseRemote, SyncError};
use super::{state_get, state_set, synced_table, CURSOR, INITIALIZED_FOR, LAST_SYNCED_AT, SYNCED_SETTINGS, SYNC_TABLES};
use crate::account::{auth, keys};
use crate::backend::{self, Backend};
use crate::db::{items, now_ms, settings, Db};
use crate::error::AppResult;
use crate::events::{self, PetFlags, PetReaction, SettingsChanged};
use crate::export;
use crate::model::{SyncState, SyncStatus};
use crate::paths::Paths;

/// Rows per push.
const BATCH: usize = 100;
/// Rows per pull.
const PAGE: usize = 100;
const TICK: Duration = Duration::from_secs(60);
const DEBOUNCE: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(5 * 60);

/// `ORDER BY` for the outbox: the tables' push order, then oldest first.
static PUSH_ORDER: LazyLock<String> = LazyLock::new(|| {
    let arms: String = SYNC_TABLES.iter().map(|(t, _, order)| format!(" WHEN '{t}' THEN {order}")).collect();
    format!("CASE tbl{arms} ELSE {} END, queued_at, tbl, id", SYNC_TABLES.len())
});

pub struct SyncEngine {
    notify: Notify,
    state: Mutex<SyncState>,
    /// Held for a whole run: the loop and `sync_now` never overlap.
    running: tokio::sync::Mutex<()>,
}

impl SyncEngine {
    fn state(&self) -> std::sync::MutexGuard<'_, SyncState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// What a run reports while it goes.
pub(crate) enum Progress {
    /// Rows are being pushed or pulled.
    Moving,
    /// A pulled page changed these rows.
    Applied(Applied),
    /// A downloaded file arrived for these items.
    Arrived(Vec<String>),
}

/// What a run works on. `user_id` is the signed-in user.
pub(crate) struct Ctx<'a> {
    pub db: &'a Db,
    pub paths: &'a Paths,
    pub keys: &'a Keys,
    pub user_id: &'a str,
}

// ---------- app ----------

/// Manages `SyncEngine` and spawns the loop.
pub fn start(app: &AppHandle) {
    app.manage(SyncEngine { notify: Notify::new(), state: Mutex::new(SyncState::Off), running: tokio::sync::Mutex::const_new(()) });
    let app = app.clone();
    tauri::async_runtime::spawn(async move { run_loop(app).await });
}

/// Wakes the loop: something synced changed locally, or the account or Local-only mode did.
pub fn poke(app: &AppHandle) {
    if let Some(engine) = app.try_state::<SyncEngine>() {
        engine.notify.notify_one();
    }
}

pub fn status(app: &AppHandle) -> AppResult<SyncStatus> {
    let state = *app.state::<SyncEngine>().state();
    read_status(&app.state::<Db>().conn(), state)
}

/// Runs a sync after the one in progress, if any; ends with `pet-reaction` `cheer` when it
/// synced, `confused` otherwise.
pub async fn sync_now(app: &AppHandle) -> AppResult<SyncStatus> {
    let state = run_once(app, true).await;
    read_status(&app.state::<Db>().conn(), state)
}

pub fn last_synced_at(conn: &Connection) -> AppResult<Option<i64>> {
    Ok(state_get(conn, LAST_SYNCED_AT)?.and_then(|v| v.parse().ok()))
}

/// After Delete cloud data: forgets this Mac's sync progress (`sync_state`, `sync_blobs`,
/// `sync_outbox`), after any run in progress, so a later account starts from scratch.
pub async fn forget(app: &AppHandle) -> AppResult<()> {
    let engine = app.state::<SyncEngine>();
    let _turn = engine.running.lock().await;
    app.state::<Db>().conn().execute_batch("DELETE FROM sync_state; DELETE FROM sync_blobs; DELETE FROM sync_outbox;")?;
    Ok(())
}

fn read_status(conn: &Connection, state: SyncState) -> AppResult<SyncStatus> {
    Ok(SyncStatus {
        state,
        last_synced_at: last_synced_at(conn)?,
        pending: conn.query_row("SELECT count(*) FROM sync_outbox", [], |r| r.get(0))?,
        blobs_pending: conn.query_row("SELECT count(*) FROM sync_blobs WHERE state IN ('up', 'down')", [], |r| r.get(0))?,
    })
}

/// Stores `state` and sends `sync-status` when it changed or a run just synced.
fn set_state(app: &AppHandle, state: SyncState) {
    let changed = std::mem::replace(&mut *app.state::<SyncEngine>().state(), state) != state;
    if !changed && state != SyncState::Idle {
        return;
    }
    match read_status(&app.state::<Db>().conn(), state) {
        Ok(s) => events::sync_status(app, s),
        Err(e) => eprintln!("[sync] status: {e}"),
    }
}

async fn run_loop(app: AppHandle) {
    let engine = app.state::<SyncEngine>();
    let mut backoff = TICK;
    loop {
        let state = run_once(&app, false).await;
        let wait = match state {
            SyncState::Idle | SyncState::Error => Some(TICK),
            SyncState::Offline => Some(backoff),
            SyncState::Off | SyncState::Paused | SyncState::NeedsKey | SyncState::SignedOut | SyncState::Syncing => None,
        };
        backoff = if state == SyncState::Offline { (backoff * 2).min(MAX_BACKOFF) } else { TICK };
        match wait {
            Some(d) => {
                tokio::select! {
                    _ = tokio::time::sleep(d) => continue,
                    _ = engine.notify.notified() => {}
                }
            }
            None => engine.notify.notified().await,
        }
        tokio::time::sleep(DEBOUNCE).await;
        // A poke during the debounce is covered by the run that follows.
        let _ = tokio::time::timeout(Duration::ZERO, engine.notify.notified()).await;
    }
}

/// Runs once (after a run in progress) and returns the state it ended in.
async fn run_once(app: &AppHandle, manual: bool) -> SyncState {
    let engine = app.state::<SyncEngine>();
    let _turn = engine.running.lock().await;
    let state = match ready(app) {
        Ok(ready) => {
            set_state(app, SyncState::Syncing);
            run_ready(app, ready).await
        }
        Err(state) => state,
    };
    set_state(app, state);
    if manual {
        events::pet_reaction(app, PetReaction::new(if state == SyncState::Idle { "cheer" } else { "confused" }));
    }
    state
}

struct Ready {
    b: Backend,
    user_id: String,
    keys: Keys,
}

/// What a run needs, or the state to show instead. No network; while not configured or signed
/// out not even the database.
fn ready(app: &AppHandle) -> Result<Ready, SyncState> {
    let fail = |what: &str, e: crate::error::AppError| {
        eprintln!("[sync] {what}: {e}");
        SyncState::Error
    };
    let b = backend::config().ok_or(SyncState::Off)?;
    let session = auth::stored_session().map_err(|e| fail("session", e))?.ok_or(SyncState::Off)?;
    if settings::read::<bool>(app, "privacy.localOnly").map_err(|e| fail("settings", e))? {
        return Err(SyncState::Paused);
    }
    let mk = keys::master_key(app).map_err(|e| fail("sync key", e))?.ok_or(SyncState::NeedsKey)?;
    Ok(Ready { b, user_id: session.user_id, keys: Keys::derive(&mk) })
}

async fn run_ready(app: &AppHandle, ready: Ready) -> SyncState {
    let remote = SupabaseRemote::new(ready.b, app.clone());
    let (db, paths) = (app.state::<Db>(), app.state::<Paths>());
    let cx = Ctx { db: &db, paths: &paths, keys: &ready.keys, user_id: &ready.user_id };
    let mut carrying = false;
    let result = sync_with(&cx, &remote, &mut |p| match p {
        Progress::Moving => {
            if !carrying {
                carrying = true;
                events::pet_flags(app, PetFlags { carrying: Some(true), ..Default::default() });
            }
        }
        Progress::Applied(applied) => emit_applied(app, applied),
        Progress::Arrived(ids) => {
            events::items_changed(app, ids);
            items::emit_counts(app);
        }
    })
    .await;
    if carrying {
        events::pet_flags(app, PetFlags { carrying: Some(false), ..Default::default() });
    }
    match result {
        Ok(()) => SyncState::Idle,
        Err(SyncError::Offline) => SyncState::Offline,
        Err(SyncError::SignedOut) => SyncState::SignedOut,
        Err(SyncError::Server(message)) => {
            eprintln!("[sync] {message}");
            SyncState::Error
        }
    }
}

/// The events for a pulled page; unlike local writes, they don't wake sync again.
fn emit_applied(app: &AppHandle, a: Applied) {
    if a.is_empty() {
        return;
    }
    if !a.item_ids.is_empty() {
        events::items_changed(app, a.item_ids);
    }
    if !a.task_ids.is_empty() {
        events::tasks_changed(app, a.task_ids);
    }
    if a.categories {
        events::categories_changed(app);
    }
    for key in a.settings_keys {
        events::emit(app, events::SETTINGS_CHANGED, SettingsChanged { key });
    }
    if a.exports {
        events::exports_changed(app, Vec::new());
    }
    items::emit_counts(app);
}

// ---------- one run ----------

/// One run without the app: queue everything on a user's first run, push, pull, move files,
/// then record `lastSyncedAt`. No database lock is held across an `.await` or during `on`.
pub(crate) async fn sync_with<R: Remote>(cx: &Ctx<'_>, remote: &R, on: &mut impl FnMut(Progress)) -> Result<(), SyncError> {
    let device_id = {
        let mut conn = cx.db.conn();
        first_run(&mut conn, cx.user_id)?;
        export::device_id(&conn)?
    };
    push(cx, remote, &device_id, on).await?;
    pull(cx, remote, on).await?;
    blobs::run(cx, remote, on).await?;
    state_set(&cx.db.conn(), LAST_SYNCED_AT, &now_ms().to_string())?;
    Ok(())
}

/// The first run for `user_id` queues every synced row (settings: the synced keys) and pulls
/// from the start.
fn first_run(conn: &mut Connection, user_id: &str) -> AppResult<()> {
    if state_get(conn, INITIALIZED_FOR)?.as_deref() == Some(user_id) {
        return Ok(());
    }
    let tx = conn.transaction()?;
    let now = now_ms();
    for (tbl, key, _) in SYNC_TABLES {
        let only = if tbl == "settings" { " WHERE key IN (SELECT value FROM json_each(?2))" } else { "" };
        let mut stmt = tx.prepare(&format!("INSERT OR IGNORE INTO sync_outbox (tbl, id, queued_at) SELECT '{tbl}', {key}, ?1 FROM {tbl}{only}"))?;
        if only.is_empty() {
            stmt.execute([now])?;
        } else {
            stmt.execute(params![now, serde_json::to_string(&SYNCED_SETTINGS)?])?;
        }
    }
    state_set(&tx, CURSOR, "0")?;
    state_set(&tx, INITIALIZED_FOR, user_id)?;
    tx.commit()?;
    Ok(())
}

struct Entry {
    tbl: String,
    id: String,
    queued_at: i64,
}

/// Pushes what was queued before the push began; a row queued again meanwhile (changed during
/// the push) keeps its entry for the next run. Pushed items' files are registered for upload.
async fn push<R: Remote>(cx: &Ctx<'_>, remote: &R, device_id: &str, on: &mut impl FnMut(Progress)) -> Result<(), SyncError> {
    let since = now_ms();
    loop {
        let (entries, rows, item_ids) = {
            let conn = cx.db.conn();
            let entries = outbox_batch(&conn, since)?;
            if entries.is_empty() {
                return Ok(());
            }
            let mut rows = Vec::with_capacity(entries.len());
            let mut item_ids = Vec::new();
            let mut pushed = Vec::with_capacity(entries.len());
            for e in entries {
                let found = match synced_table(&e.tbl) {
                    Some((tbl, key)) => read_row(&conn, tbl, key, &e.id)?.map(|row| (tbl, row)),
                    None => None,
                };
                let Some((tbl, row)) = found else {
                    // The row is gone: nothing to send.
                    delete_entry(&conn, &e)?;
                    continue;
                };
                if tbl == "items" {
                    item_ids.push(e.id.clone());
                }
                rows.push(RemoteRow {
                    tbl: e.tbl.clone(),
                    id: e.id.clone(),
                    updated_at: row.get("updated_at").and_then(Value::as_i64).unwrap_or(0),
                    device_id: device_id.to_string(),
                    deleted: row.get("deleted_at").is_some_and(|v| !v.is_null()),
                    payload: seal_row(&cx.keys.record, tbl, &e.id, &row),
                    seq: 0,
                });
                pushed.push(e);
            }
            (pushed, rows, item_ids)
        };
        if rows.is_empty() {
            continue;
        }
        on(Progress::Moving);
        remote.push(&rows).await?;
        let mut conn = cx.db.conn();
        let tx = conn.transaction()?;
        for e in &entries {
            delete_entry(&tx, e)?;
        }
        blobs::register(&tx, cx.paths, &item_ids, now_ms())?;
        tx.commit()?;
    }
}

fn outbox_batch(conn: &Connection, since: i64) -> AppResult<Vec<Entry>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT tbl, id, queued_at FROM sync_outbox WHERE queued_at <= ?1 ORDER BY {} LIMIT ?2",
        *PUSH_ORDER
    ))?;
    let entries = stmt
        .query_map(params![since, BATCH as i64], |r| Ok(Entry { tbl: r.get(0)?, id: r.get(1)?, queued_at: r.get(2)? }))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(entries)
}

/// Deletes the entry unless the row was queued again since it was read.
fn delete_entry(conn: &Connection, e: &Entry) -> AppResult<()> {
    conn.prepare_cached("DELETE FROM sync_outbox WHERE tbl = ?1 AND id = ?2 AND queued_at = ?3")?
        .execute(params![e.tbl, e.id, e.queued_at])?;
    Ok(())
}

/// Pulls and applies pages from `cursor` until a page comes back short.
async fn pull<R: Remote>(cx: &Ctx<'_>, remote: &R, on: &mut impl FnMut(Progress)) -> Result<(), SyncError> {
    loop {
        let cursor: i64 = state_get(&cx.db.conn(), CURSOR)?.and_then(|c| c.parse().ok()).unwrap_or(0);
        let rows = remote.pull(cursor, PAGE).await?;
        if rows.is_empty() {
            return Ok(());
        }
        on(Progress::Moving);
        let applied = apply_page(&mut cx.db.conn(), &rows, &cx.keys.record, cx.paths)?;
        on(Progress::Applied(applied));
        if rows.len() < PAGE {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use serde_json::json;

    use super::*;
    use crate::db::{categories, tasks};
    use crate::model::{Actor, ItemQuery, NewItem, NewTask};
    use crate::sync::remote::MemoryRemote;
    use crate::sync::testing::{device, keys, outbox_len, sync, sync_device, Device};

    fn add_note(d: &Device, title: &str, body: &str) -> String {
        let new = NewItem {
            kind: "text".into(),
            title: title.into(),
            payload_text: Some(body.into()),
            source_kind: "note".into(),
            content_hash: format!("hash-{title}"),
            ..Default::default()
        };
        items::insert_item(&mut d.db.conn(), &new).unwrap()
    }

    fn add_task(d: &Device, title: &str) -> String {
        let new = NewTask { title: title.into(), ..Default::default() };
        tasks::create(&mut d.db.conn(), &new, &Actor::User).unwrap().0.id
    }

    fn task_title(d: &Device, id: &str) -> String {
        d.db.conn().query_row("SELECT title FROM tasks WHERE id = ?1", [id], |r| r.get(0)).unwrap()
    }

    /// A local edit at a chosen `updated_at` (so the tests control who is later).
    fn edit_task(d: &Device, id: &str, title: &str, at: i64) {
        d.db.conn().execute("UPDATE tasks SET title = ?2, updated_at = ?3 WHERE id = ?1", params![id, title, at]).unwrap();
    }

    fn search(d: &Device, q: &str) -> Vec<String> {
        let query = ItemQuery {
            view: "all".into(),
            category_id: None,
            search: Some(q.into()),
            sort: None,
            limit: 50,
            cursor: None,
            agent_visible: false,
            captured_since: None,
        };
        items::list_items(&d.db.conn(), &query, &d.paths).unwrap().items.into_iter().map(|i| i.id).collect()
    }

    #[tokio::test]
    async fn two_devices_converge() {
        let (a, b, keys, remote) = (device("aaaa"), device("bbbb"), keys(), MemoryRemote::default());
        let item = add_note(&a, "Đường đến trạm", "bus timetable");
        let task = add_task(&a, "Call the plumber");
        let cat = categories::create(&mut a.db.conn(), "Receipts", "file", "todo").unwrap();
        sync(&a, &remote, &keys).await;
        sync(&b, &remote, &keys).await;

        assert_eq!(items::get_row(&b.db.conn(), &item).unwrap().title, "Đường đến trạm");
        assert_eq!(task_title(&b, &task), "Call the plumber");
        let b_cats = categories::list(&b.db.conn()).unwrap();
        assert!(b_cats.iter().any(|c| c.id == cat.id && c.name.as_deref() == Some("Receipts")));
        assert_eq!(search(&b, "duong"), [item.clone()], "the pulled item is in B's search index");
        assert_eq!(search(&b, "timetable"), [item]);
        // Both list the same categories in the same order.
        assert_eq!(categories::list(&a.db.conn()).unwrap().len(), b_cats.len());
        sync(&a, &remote, &keys).await;
        let ids = |d: &Device| categories::list(&d.db.conn()).unwrap().into_iter().map(|c| (c.id, c.position)).collect::<Vec<_>>();
        assert_eq!(ids(&a), ids(&b));
    }

    #[tokio::test]
    async fn last_writer_wins_both_orders() {
        // (A's and B's edit, ms after the task was created; the title both end with). Equal
        // times: the greater device id wins ("bbbb" > "aaaa"), as on the server.
        for (a_after, b_after, winner) in [(1_000, 2_000, "B"), (2_000, 1_000, "A"), (1_500, 1_500, "B")] {
            for a_first in [true, false] {
                let (a, b, keys, remote) = (device("aaaa"), device("bbbb"), keys(), MemoryRemote::default());
                let id = add_task(&a, "original");
                sync(&a, &remote, &keys).await;
                sync(&b, &remote, &keys).await;
                let created = a.db.conn().query_row("SELECT updated_at FROM tasks WHERE id = ?1", [&id], |r| r.get::<_, i64>(0)).unwrap();
                edit_task(&a, &id, "A", created + a_after);
                edit_task(&b, &id, "B", created + b_after);
                let (first, second) = if a_first { (&a, &b) } else { (&b, &a) };
                sync(first, &remote, &keys).await;
                sync(second, &remote, &keys).await;
                sync(first, &remote, &keys).await;
                let case = format!("A +{a_after} ms, B +{b_after} ms, A first {a_first}");
                assert_eq!(task_title(&a, &id), winner, "{case}");
                assert_eq!(task_title(&b, &id), winner, "{case}");
                assert_eq!(outbox_len(&a.db.conn()), 0, "{case}");
                assert_eq!(outbox_len(&b.db.conn()), 0, "{case}");
            }
        }
    }

    #[tokio::test]
    async fn pulled_rows_do_not_echo() {
        let (a, b, keys, remote) = (device("aaaa"), device("bbbb"), keys(), MemoryRemote::default());
        add_note(&a, "Pulled once", "");
        add_task(&a, "Pulled task");
        sync(&a, &remote, &keys).await;
        sync(&b, &remote, &keys).await;
        assert_eq!(outbox_len(&b.db.conn()), 0, "applying pulled rows queues nothing");
        let rows = remote.rows();
        sync(&b, &remote, &keys).await;
        sync(&a, &remote, &keys).await;
        assert_eq!(remote.rows(), rows, "nothing was pushed back");
    }

    #[tokio::test]
    async fn device_settings_stay_local() {
        let (a, b, keys, remote) = (device("aaaa"), device("bbbb"), keys(), MemoryRemote::default());
        settings::put(&a.db.conn(), "pet.name", &json!("Mochi")).unwrap();
        settings::put(&a.db.conn(), "hotkeys", &json!({ "focusPet": "F1" })).unwrap();
        settings::put(&a.db.conn(), "privacy.localOnly", &json!(true)).unwrap();
        sync(&a, &remote, &keys).await;
        assert!(remote.row("settings", "pet.name").is_some());
        let synced: Vec<String> = remote.rows().into_iter().filter(|r| r.tbl == "settings").map(|r| r.id).collect();
        assert!(synced.iter().all(|k| SYNCED_SETTINGS.contains(&k.as_str())), "{synced:?}");
        assert_eq!(synced.len(), SYNCED_SETTINGS.len());

        sync(&b, &remote, &keys).await;
        let c = b.db.conn();
        assert_eq!(settings::get::<String>(&c, "pet.name").unwrap(), "Mochi");
        assert_ne!(settings::get::<Value>(&c, "hotkeys").unwrap()["focusPet"], "F1");
        assert!(!settings::get::<bool>(&c, "privacy.localOnly").unwrap());
        assert_eq!(export::device_id(&c).unwrap(), "bbbb");
    }

    #[tokio::test]
    async fn server_sees_only_ciphertext() {
        let (a, keys, remote) = (device("aaaa"), keys(), MemoryRemote::default());
        add_note(&a, "Zebra quarterly budget", "Zebra numbers inside");
        add_task(&a, "Zebra follow-up");
        sync(&a, &remote, &keys).await;
        let rows = remote.rows();
        assert!(rows.iter().any(|r| r.tbl == "items"));
        for r in rows {
            assert!(!r.payload.contains("Zebra"));
            let sealed = STANDARD.decode(&r.payload).unwrap();
            assert!(!sealed.windows(5).any(|w| w == b"Zebra"), "{}/{} leaks plaintext", r.tbl, r.id);
        }
    }

    #[tokio::test]
    async fn wrong_key_skips_rows() {
        let (a, b, remote) = (device("aaaa"), device("bbbb"), MemoryRemote::default());
        let item = add_note(&a, "Secret plan", "");
        sync(&a, &remote, &keys()).await;
        let other = Keys::derive(&[8; 32]);
        sync(&b, &remote, &other).await;
        assert!(items::get_row(&b.db.conn(), &item).is_err(), "B can't read A's rows");
        let cursor = state_get(&b.db.conn(), CURSOR).unwrap().unwrap();
        assert_eq!(cursor, remote.rows().last().unwrap().seq.to_string(), "undecryptable rows are passed over");
    }

    /// Changes a task while its push is in flight.
    struct EditDuringPush<'a> {
        inner: &'a MemoryRemote,
        device: &'a Device,
        task: &'a str,
    }

    impl Remote for EditDuringPush<'_> {
        async fn push(&self, rows: &[RemoteRow]) -> Result<u32, SyncError> {
            if rows.iter().any(|r| r.id == self.task) {
                edit_task(self.device, self.task, "edited during push", now_ms() + 5);
            }
            self.inner.push(rows).await
        }
        async fn pull(&self, after: i64, limit: usize) -> Result<Vec<RemoteRow>, SyncError> {
            self.inner.pull(after, limit).await
        }
        async fn put_blob(&self, name: &str, bytes: Vec<u8>) -> Result<(), SyncError> {
            self.inner.put_blob(name, bytes).await
        }
        async fn get_blob(&self, name: &str) -> Result<Option<Vec<u8>>, SyncError> {
            self.inner.get_blob(name).await
        }
    }

    #[tokio::test]
    async fn changed_during_push_stays_queued() {
        let (a, b, keys, memory) = (device("aaaa"), device("bbbb"), keys(), MemoryRemote::default());
        let task = add_task(&a, "before");
        let queued_before: i64 = a.db.conn().query_row("SELECT queued_at FROM sync_outbox WHERE id = ?1", [&task], |r| r.get(0)).unwrap();
        sync_device(&a, &EditDuringPush { inner: &memory, device: &a, task: &task }, &keys).await.unwrap();
        let queued: Vec<(String, i64)> = {
            let c = a.db.conn();
            let mut st = c.prepare("SELECT id, queued_at FROM sync_outbox").unwrap();
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect()
        };
        assert_eq!(queued.len(), 1, "only the edited row stays queued: {queued:?}");
        assert_eq!(queued[0].0, task);
        assert!(queued[0].1 >= queued_before);
        assert_eq!(task_title(&a, &task), "edited during push", "the stale echo didn't overwrite it");

        sync(&a, &memory, &keys).await;
        assert_eq!(outbox_len(&a.db.conn()), 0);
        sync(&b, &memory, &keys).await;
        assert_eq!(task_title(&b, &task), "edited during push");
    }

    #[tokio::test]
    async fn first_run_for_a_user_queues_everything_once() {
        let (a, keys, remote) = (device("aaaa"), keys(), MemoryRemote::default());
        sync(&a, &remote, &keys).await;
        let seeded = remote.rows().len();
        let categories: i64 = a.db.conn().query_row("SELECT count(*) FROM categories", [], |r| r.get(0)).unwrap();
        let rules: i64 = a.db.conn().query_row("SELECT count(*) FROM rules", [], |r| r.get(0)).unwrap();
        assert_eq!(seeded as i64, categories + rules + SYNCED_SETTINGS.len() as i64);
        sync(&a, &remote, &keys).await;
        assert_eq!(remote.rows().len(), seeded, "the second run queues nothing new");
        assert!(last_synced_at(&a.db.conn()).unwrap().is_some());
    }

    /// A user signed up (or signed in) with a password on the local stack.
    async fn password_session(b: &Backend, path: &str, email: &str) -> crate::account::auth::Session {
        let body = json!({ "email": email, "password": "e2e-Password-1" });
        let resp = backend::http().post(format!("{}/auth/v1/{path}", b.url)).header("apikey", b.key).json(&body).send().await.unwrap();
        let v: Value = resp.error_for_status().unwrap().json().await.unwrap();
        crate::account::auth::Session {
            access_token: v["access_token"].as_str().unwrap().into(),
            refresh_token: v["refresh_token"].as_str().unwrap().into(),
            expires_at: v["expires_at"].as_i64().unwrap() * 1000,
            user_id: v["user"]["id"].as_str().unwrap().into(),
            email: Some(email.into()),
        }
    }

    /// Two in-memory Macs syncing through a local Supabase stack with the real `SupabaseRemote`
    /// (a fixed session stands in for the keychain). Not run by default:
    /// `COPET_E2E_URL=http://127.0.0.1:<api port> COPET_E2E_KEY=<publishable/anon key>
    ///  cargo test --lib sync::engine::tests::local_stack_end_to_end -- --ignored --nocapture`
    /// (needs the `sync_keys` and `sync_records` migrations; email confirmations off).
    #[tokio::test]
    #[ignore = "needs a local Supabase stack (COPET_E2E_URL, COPET_E2E_KEY)"]
    async fn local_stack_end_to_end() {
        use crate::account::crypto::{crockford_encode, random32};
        use crate::account::keys as account_keys;
        use crate::sync::codec::blob_name;
        use crate::sync::testing::file_item;

        let env = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set"));
        let (url, key): (&'static str, &'static str) = (env("COPET_E2E_URL").leak(), env("COPET_E2E_KEY").leak());
        let backend = || Backend { url, key };
        let email = format!("sync-e2e-{}@example.com", hex::encode(&random32()[..6]));

        // Mac A signs up and turns sync on; Mac B signs in and unlocks with the recovery key.
        let sa = password_session(&backend(), "signup", &email).await;
        let (mk, rk) = (random32(), random32());
        account_keys::upload(&backend(), &sa, &mk, &rk).await.unwrap();
        let sb = password_session(&backend(), "token?grant_type=password", &email).await;
        assert_eq!(sb.user_id, sa.user_id);
        let mk_b = account_keys::recover(&backend(), &sb, &crockford_encode(&rk)).await.unwrap();
        assert_eq!(mk_b, mk);
        let (keys_a, keys_b) = (Keys::derive(&mk), Keys::derive(&mk_b));
        println!("user {}: A uploaded the wrapped key; B recovered the same master key", sa.user_id);

        let (a, b) = (device("aaaa"), device("bbbb"));
        let title = "Zebra invoice from Hà Nội";
        let item = add_note(&a, title, "Zebra total 1,250,000 VND");
        let task = add_task(&a, "Pay the zebra invoice");
        let cat = categories::create(&mut a.db.conn(), "Invoices", "file", "todo").unwrap();
        let file_bytes = b"%PDF-1.7 zebra file body".repeat(32);
        let (file_id, rel) = file_item(&a, &file_bytes, "pdf");
        let remote_a = SupabaseRemote::new(backend(), sa.clone());
        let remote_b = SupabaseRemote::new(backend(), sb.clone());
        let sync_a = || async { sync_device(&a, &remote_a, &keys_a).await.unwrap() };
        let sync_b = || async { sync_device(&b, &remote_b, &keys_b).await.unwrap() };
        sync_a().await;
        assert_eq!(outbox_len(&a.db.conn()), 0);
        sync_b().await;
        assert_eq!(items::get_row(&b.db.conn(), &item).unwrap().title, title);
        assert_eq!(task_title(&b, &task), "Pay the zebra invoice");
        assert!(categories::list(&b.db.conn()).unwrap().iter().any(|c| c.id == cat.id));
        assert_eq!(search(&b, "ha noi"), [item.clone()]);
        assert_eq!(std::fs::read(b.paths.abs(&rel)).unwrap(), file_bytes);
        assert!(items::get_item_detail(&b.db.conn(), &file_id, &b.paths).unwrap().item.file_available);
        assert_eq!(outbox_len(&b.db.conn()), 0);
        println!("A -> B: item, task, category and the PDF arrived; B's search finds the item; outboxes empty");

        // What the server holds: ciphertext rows and HMAC-named objects.
        let rows: Vec<RemoteRow> = remote_a.pull(0, 1000).await.unwrap();
        assert!(rows.iter().any(|r| r.tbl == "items" && r.id == item));
        for r in &rows {
            let sealed = STANDARD.decode(&r.payload).unwrap();
            assert!(!r.payload.contains("Zebra") && !sealed.windows(5).any(|w| w == b"Zebra"), "{}/{} leaks", r.tbl, r.id);
        }
        let listed: Value = backend::http()
            .post(format!("{url}/storage/v1/object/list/blobs"))
            .header("apikey", key)
            .bearer_auth(&sa.access_token)
            .json(&json!({ "prefix": sa.user_id, "limit": 100 }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let names: Vec<&str> = listed.as_array().unwrap().iter().map(|o| o["name"].as_str().unwrap()).collect();
        assert_eq!(names, [blob_name(&keys_a.name, &rel).as_str()]);
        let stored = remote_b.get_blob(names[0]).await.unwrap().unwrap();
        assert!(!stored.windows(5).any(|w| w == b"zebra"));
        assert_eq!(remote_b.get_blob(&"0".repeat(64)).await.unwrap(), None);
        println!("server: {} records, none containing the title; objects under {}/: {names:?} (sealed, {} bytes)", rows.len(), sa.user_id, stored.len());

        // Same updated_at on both Macs: server and clients agree on the device-id tie-break.
        let at = crate::db::now_ms() + 1_000;
        edit_task(&a, &task, "edited on A", at);
        edit_task(&b, &task, "edited on B", at);
        sync_a().await;
        sync_b().await;
        sync_a().await;
        assert_eq!(task_title(&a, &task), "edited on B");
        assert_eq!(task_title(&b, &task), "edited on B");
        let server = remote_a.pull(0, 1000).await.unwrap().into_iter().find(|r| r.id == task).unwrap();
        assert_eq!((server.updated_at, server.device_id.as_str()), (at, "bbbb"));
        println!("equal updated_at edits: both Macs and the server keep device bbbb's");

        // A token the server refuses (and that can't be renewed) is `SignedOut`, from PostgREST
        // (401) and from Storage (400 "Unauthorized") alike.
        let refused = SupabaseRemote::new(backend(), crate::account::auth::Session { access_token: "x.y.z".into(), ..sa.clone() });
        assert_eq!(refused.pull(0, 1).await, Err(SyncError::SignedOut));
        assert_eq!(refused.push(&[]).await, Err(SyncError::SignedOut));
        assert_eq!(refused.get_blob(names[0]).await, Err(SyncError::SignedOut));
        assert_eq!(refused.put_blob(names[0], vec![1]).await, Err(SyncError::SignedOut));
        println!("refused token: pull, push, get_blob, put_blob -> SignedOut");
    }
}
