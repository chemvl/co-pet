//! The export worker: one job at a time, only this device's jobs, sleeping until the next due
//! job or a `poke`. Paused (jobs stay queued) while `privacy.localOnly` is on.
use std::str::FromStr;
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};
use tauri::{AppHandle, Manager};
use tokio::sync::Notify;

use super::{ExportError, ExportPayload, Exported, Provider};
use crate::db::{now_ms, settings, Db};
use crate::error::AppResult;
use crate::events::{self, PetFlags, PetReaction};
use crate::i18n;
use crate::model::ExportTarget;
use crate::paths::Paths;

/// The longest sleep without a due job.
const MAX_SLEEP_MS: i64 = 60 * 60 * 1000;
const FIRST_DELAY_MS: i64 = 30_000;
const MAX_DELAY_MS: i64 = 15 * 60 * 1000;
/// Network failures before a job fails.
const MAX_ATTEMPTS: i64 = 6;

pub struct ExportQueue {
    notify: Notify,
}

impl ExportQueue {
    /// Wakes the worker (new job, retry, localOnly turned off).
    pub fn poke(&self) {
        self.notify.notify_one();
    }
}

#[derive(Debug)]
struct Job {
    id: String,
    task_id: String,
    provider: String,
    attempts: i64,
}

enum Step {
    LocalOnly,
    Run(Job),
    /// Milliseconds until the next due job (capped at 1 h).
    Wait(i64),
}

/// The retry delay after `attempts` network failures: 30 s doubling, capped at 15 min.
pub fn next_delay(attempts: i64) -> i64 {
    let doublings = (attempts - 1).clamp(0, 16) as u32;
    FIRST_DELAY_MS.saturating_mul(1 << doublings).min(MAX_DELAY_MS)
}

/// The row a job outcome writes.
#[derive(Debug, PartialEq)]
struct Update {
    state: &'static str,
    attempts: i64,
    next_attempt_at: Option<i64>,
    error: Option<&'static str>,
    exported: Option<Exported>,
}

fn outcome(attempts: i64, result: Result<Exported, ExportError>, now: i64) -> Update {
    let failed = |error| Update { state: "failed", attempts, next_attempt_at: None, error: Some(error), exported: None };
    match result {
        Ok(e) => Update { state: "exported", attempts, next_attempt_at: None, error: None, exported: Some(e) },
        Err(ExportError::RateLimited(secs)) => Update {
            state: "queued",
            attempts: attempts + 1,
            next_attempt_at: Some(now + (secs as i64).saturating_mul(1000)),
            error: Some("rateLimited"),
            exported: None,
        },
        Err(ExportError::Retryable) => {
            let attempts = attempts + 1;
            if attempts >= MAX_ATTEMPTS {
                Update { attempts, ..failed("network") }
            } else {
                Update {
                    state: "queued",
                    attempts,
                    next_attempt_at: Some(now + next_delay(attempts)),
                    error: Some("network"),
                    exported: None,
                }
            }
        }
        Err(ExportError::Unauthorized) => failed("unauthorized"),
        Err(ExportError::TargetMissing) => failed("targetMissing"),
        Err(ExportError::Invalid(_)) => failed("invalid"),
    }
}

/// After a crash or quit mid-job: this device's `exporting` rows go back to `queued`.
fn reset_exporting(conn: &Connection, device: &str, now: i64) -> AppResult<usize> {
    Ok(conn.execute(
        "UPDATE task_exports SET state = 'queued', updated_at = ?2
         WHERE device_id = ?1 AND state = 'exporting' AND deleted_at IS NULL",
        params![device, now],
    )?)
}

fn next_step(conn: &Connection, now: i64) -> AppResult<Step> {
    if settings::get::<bool>(conn, "privacy.localOnly")? {
        return Ok(Step::LocalOnly);
    }
    let device = super::device_id(conn)?;
    let job = conn
        .query_row(
            "SELECT id, task_id, provider, attempts FROM task_exports
             WHERE state = 'queued' AND device_id = ?1 AND deleted_at IS NULL
               AND (next_attempt_at IS NULL OR next_attempt_at <= ?2)
             ORDER BY COALESCE(next_attempt_at, created_at), created_at LIMIT 1",
            params![device, now],
            |r| Ok(Job { id: r.get(0)?, task_id: r.get(1)?, provider: r.get(2)?, attempts: r.get(3)? }),
        )
        .optional()?;
    if let Some(job) = job {
        return Ok(Step::Run(job));
    }
    let next: Option<i64> = conn.query_row(
        "SELECT MIN(next_attempt_at) FROM task_exports
         WHERE state = 'queued' AND device_id = ?1 AND deleted_at IS NULL AND next_attempt_at > ?2",
        params![device, now],
        |r| r.get(0),
    )?;
    Ok(Step::Wait(next.map_or(MAX_SLEEP_MS, |at| (at - now).clamp(0, MAX_SLEEP_MS))))
}

/// Manages `ExportQueue`, requeues interrupted jobs and spawns the worker.
pub fn start(app: &AppHandle) {
    app.manage(ExportQueue { notify: Notify::new() });
    {
        let db = app.state::<Db>();
        let conn = db.conn();
        if let Err(e) = super::device_id(&conn).and_then(|d| reset_exporting(&conn, &d, now_ms())) {
            eprintln!("[export] requeue interrupted jobs: {e}");
        }
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move { run(app).await });
}

fn carrying(app: &AppHandle, on: bool) {
    events::pet_flags(app, PetFlags { carrying: Some(on), ..Default::default() });
}

async fn run(app: AppHandle) {
    let queue = app.state::<ExportQueue>();
    let mut busy = false;
    loop {
        let step = next_step(&app.state::<Db>().conn(), now_ms());
        match step {
            Ok(Step::Run(job)) => {
                if !busy {
                    busy = true;
                    carrying(&app, true);
                }
                run_job(&app, job).await;
                continue;
            }
            Ok(Step::LocalOnly) | Ok(Step::Wait(_)) | Err(_) if busy => {
                busy = false;
                carrying(&app, false);
            }
            _ => {}
        }
        match step {
            Ok(Step::LocalOnly) => queue.notify.notified().await,
            Ok(Step::Wait(ms)) => {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(ms as u64)) => {}
                    _ = queue.notify.notified() => {}
                }
            }
            Err(e) => {
                eprintln!("[export] queue: {e}");
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(60)) => {}
                    _ = queue.notify.notified() => {}
                }
            }
            Ok(Step::Run(_)) => unreachable!(),
        }
    }
}

/// Marks the job `exporting` and loads what it sends.
fn prepare(app: &AppHandle, job: &Job, lang: i18n::Lang) -> Result<(Provider, ExportTarget, ExportPayload), ExportError> {
    let db = app.state::<Db>();
    let conn = db.conn();
    let invalid = |e: crate::error::AppError| ExportError::Invalid(e.to_string());
    conn.execute(
        "UPDATE task_exports SET state = 'exporting', updated_at = ?2 WHERE id = ?1",
        params![job.id, now_ms()],
    )
    .map_err(|e| invalid(e.into()))?;
    let p = Provider::from_str(&job.provider).map_err(invalid)?;
    let target = super::target(&conn, p).map_err(invalid)?.ok_or(ExportError::TargetMissing)?;
    let payload = super::payload(&conn, lang, &app.state::<Paths>().library, &job.task_id).map_err(invalid)?;
    Ok((p, target, payload))
}

async fn run_job(app: &AppHandle, job: Job) {
    // Before taking the connection: `lang` reads a setting through the same mutex.
    let lang = i18n::lang(app);
    let prepared = prepare(app, &job, lang);
    events::exports_changed(app, vec![job.task_id.clone()]);
    let provider = prepared.as_ref().ok().map(|(p, _, _)| *p);
    let result = match prepared {
        Ok((p, target, payload)) => super::export(p, &target, &payload).await,
        Err(e) => Err(e),
    };
    if let Err(ExportError::Invalid(msg)) = &result {
        eprintln!("[export] job {} invalid: {msg}", job.id);
    }
    let update = outcome(job.attempts, result, now_ms());
    let written = {
        let db = app.state::<Db>();
        let conn = db.conn();
        write(&conn, &job.id, &update, now_ms())
    };
    if let Err(e) = written {
        eprintln!("[export] job {}: {e}", job.id);
    }
    match (update.state, update.error) {
        ("exported", _) => events::pet_reaction(app, PetReaction::new("cheer")),
        ("failed", error) => {
            if error == Some("unauthorized") {
                if let Some(Err(e)) = provider.map(super::mark_needs_reconnect) {
                    eprintln!("[export] mark reconnect: {e}");
                }
            }
            events::pet_reaction(app, PetReaction::new("confused"));
        }
        _ => {}
    }
    events::exports_changed(app, vec![job.task_id]);
}

fn write(conn: &Connection, id: &str, u: &Update, now: i64) -> AppResult<()> {
    conn.execute(
        "UPDATE task_exports SET state = ?2, attempts = ?3, next_attempt_at = ?4, error = ?5,
           external_id = COALESCE(?6, external_id), external_url = COALESCE(?7, external_url), updated_at = ?8
         WHERE id = ?1",
        params![
            id,
            u.state,
            u.attempts,
            u.next_attempt_at,
            u.error,
            u.exported.as_ref().map(|e| e.external_id.as_str()),
            u.exported.as_ref().map(|e| e.external_url.as_str()),
            now
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{enqueue, tests::task};

    #[test]
    fn backoff_schedule() {
        let secs: Vec<i64> = (1..=8).map(|a| next_delay(a) / 1000).collect();
        assert_eq!(secs, vec![30, 60, 120, 240, 480, 900, 900, 900]);
        assert_eq!(next_delay(100), MAX_DELAY_MS);
        let u = outcome(0, Err(ExportError::Retryable), 1_000);
        assert_eq!((u.state, u.attempts, u.next_attempt_at, u.error), ("queued", 1, Some(31_000), Some("network")));
        let u = outcome(4, Err(ExportError::Retryable), 1_000);
        assert_eq!((u.state, u.attempts, u.next_attempt_at), ("queued", 5, Some(1_000 + 480_000)));
        let u = outcome(5, Err(ExportError::Retryable), 1_000);
        assert_eq!((u.state, u.attempts, u.next_attempt_at, u.error), ("failed", 6, None, Some("network")));
    }

    #[test]
    fn rate_limit_uses_retry_after() {
        let e = crate::export::classify(429, Some("120"), String::new());
        let u = outcome(2, Err(e), 1_000);
        assert_eq!((u.state, u.attempts, u.next_attempt_at, u.error), ("queued", 3, Some(121_000), Some("rateLimited")));
        let u = outcome(0, Err(crate::export::classify(429, None, String::new())), 0);
        assert_eq!(u.next_attempt_at, Some(30_000));
    }

    #[test]
    fn terminal_outcomes() {
        let ok = Exported { external_id: "p1".into(), external_url: "https://n/p1".into() };
        let u = outcome(3, Ok(ok.clone()), 5);
        assert_eq!((u.state, u.error, u.exported), ("exported", None, Some(ok)));
        assert_eq!(outcome(0, Err(ExportError::Unauthorized), 0).error, Some("unauthorized"));
        assert_eq!(outcome(0, Err(ExportError::TargetMissing), 0).error, Some("targetMissing"));
        let u = outcome(0, Err(ExportError::Invalid("x".into())), 0);
        assert_eq!((u.state, u.error), ("failed", Some("invalid")));
    }

    #[test]
    fn exporting_reset_on_start() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        let device = crate::export::device_id(&conn).unwrap();
        assert_eq!(device.len(), 32);
        let a = task(&conn, "A");
        let b = task(&conn, "B");
        enqueue(&conn, &a, Provider::Notion, 1).unwrap();
        enqueue(&conn, &b, Provider::Notion, 1).unwrap();
        conn.execute("UPDATE task_exports SET state = 'exporting'", []).unwrap();
        conn.execute("UPDATE task_exports SET device_id = 'other' WHERE task_id = ?1", [&b]).unwrap();
        assert_eq!(reset_exporting(&conn, &device, 2).unwrap(), 1);
        let states: Vec<(String, String)> = conn
            .prepare("SELECT task_id, state FROM task_exports ORDER BY task_id = ?1 DESC")
            .unwrap()
            .query_map([&a], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(states, vec![(a, "queued".into()), (b, "exporting".into())]);
    }

    #[test]
    fn picks_due_jobs_of_this_device() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        let a = task(&conn, "A");
        enqueue(&conn, &a, Provider::Asana, 1).unwrap();
        conn.execute("UPDATE task_exports SET next_attempt_at = 10000", []).unwrap();
        assert!(matches!(next_step(&conn, 4_000).unwrap(), Step::Wait(6_000)));
        assert!(matches!(next_step(&conn, 10_000).unwrap(), Step::Run(Job { attempts: 0, .. })));
        settings::put(&conn, "privacy.localOnly", &serde_json::Value::Bool(true)).unwrap();
        assert!(matches!(next_step(&conn, 10_000).unwrap(), Step::LocalOnly));
        settings::put(&conn, "privacy.localOnly", &serde_json::Value::Bool(false)).unwrap();
        conn.execute("UPDATE task_exports SET device_id = 'other'", []).unwrap();
        assert!(matches!(next_step(&conn, 10_000).unwrap(), Step::Wait(MAX_SLEEP_MS)));
    }
}
