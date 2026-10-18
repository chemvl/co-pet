//! Proactive What's Next: a 30 s scheduler that keeps the due-soon badge dot in time and
//! offers the card when the user comes back from being away, plus the after-done offer. Every
//! offer passes [`allowed`] first; the pet window records `shown` when it displays the card.
use std::time::Duration;

use chrono::{DateTime, Datelike, Local, NaiveDateTime, Timelike};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::db::{diary, items, now_ms, settings, tasks, Db};
use crate::error::{AppError, AppResult};
use crate::events::{self, DiaryReady, WhatsNextEvent};
use crate::platform::fullscreen::Visibility;
use crate::platform::idle::{self, Sleep};
use crate::transcribe::recorder::Recorder;

const TICK: Duration = Duration::from_secs(30);
/// Input this recent after being away counts as a return.
const RETURN_SECS: f64 = 30.0;
/// The pet wakes on its own poll; an offer sent while it still sleeps would be dropped.
const WAKE_GRACE: Duration = Duration::from_secs(idle::TICK.as_secs() + 1);
const AFTER_DONE_DELAY: Duration = Duration::from_secs(2);
const HOUR_MS: i64 = 3_600_000;

/// `whatsNext.workHours`. `days` are ISO weekdays (1 = Monday … 7 = Sunday).
#[derive(Debug, Deserialize)]
struct WorkHours {
    start: String,
    end: String,
    days: Vec<u32>,
}

/// Weekday in `days` and `start <= HH:MM < end`. Unparsable times never match.
fn in_work_hours(hours: &WorkHours, now: NaiveDateTime) -> bool {
    let (Some(start), Some(end)) = (idle::minutes(&hours.start), idle::minutes(&hours.end)) else {
        return false;
    };
    let minute = now.hour() * 60 + now.minute();
    hours.days.contains(&now.weekday().number_from_monday()) && start <= minute && minute < end
}

/// Return detection: away once idle reaches the threshold; back on the first tick with input
/// in the last 30 s.
#[derive(Debug, Default)]
struct ReturnWatch {
    away: bool,
}

impl ReturnWatch {
    /// `true` exactly on the tick the user comes back.
    fn tick(&mut self, idle_secs: f64, away_secs: f64) -> bool {
        if idle_secs >= away_secs {
            self.away = true;
        } else if self.away && idle_secs < RETURN_SECS {
            self.away = false;
            return true;
        }
        false
    }
}

/// Proactive cards (`return`, `afterDone`, `afterMeeting`) shown in the hour before `now_ms`.
fn shown_last_hour(conn: &Connection, now_ms: i64) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM next_feedback
         WHERE action = 'shown' AND trigger IN ('return', 'afterDone', 'afterMeeting') AND at > ?1",
        [now_ms - HOUR_MS],
        |r| r.get(0),
    )?)
}

/// Onboarding done, inside `whatsNext.workHours`, and [`pet_shown`].
fn pet_available(conn: &Connection, now: DateTime<Local>) -> AppResult<bool> {
    if !settings::get::<bool>(conn, "onboarding.completed")?
        || !in_work_hours(&settings::get(conn, "whatsNext.workHours")?, now.naive_local())
    {
        return Ok(false);
    }
    pet_shown(conn, now.timestamp_millis())
}

/// The pet neither paused (`pet.paused`) nor hidden by the user (`pet.hiddenUntil`) at `now_ms`.
pub fn pet_shown(conn: &Connection, now_ms: i64) -> AppResult<bool> {
    if settings::get::<bool>(conn, "pet.paused")? {
        return Ok(false);
    }
    Ok(!settings::get::<Option<f64>>(conn, "pet.hiddenUntil")?.is_some_and(|until| (now_ms as f64) < until))
}

/// The settings-based guardrails for `trigger` (`return | afterDone | afterMeeting`) at `now`:
/// the trigger's switch, [`pet_available`], and fewer than `whatsNext.maxPerHour` proactive
/// cards shown in the last hour.
fn settings_allow(conn: &Connection, trigger: &str, now: DateTime<Local>) -> AppResult<bool> {
    let switch = match trigger {
        "return" => "whatsNext.onReturn",
        "afterDone" => "whatsNext.afterDone",
        "afterMeeting" => "whatsNext.afterMeeting",
        _ => return Err(AppError::Code("invalid")),
    };
    if !settings::get::<bool>(conn, switch)? || !pet_available(conn, now)? {
        return Ok(false);
    }
    Ok(shown_last_hour(conn, now.timestamp_millis())? < settings::get::<i64>(conn, "whatsNext.maxPerHour")?)
}

/// Last week's diary notice when due at `now`: `diary.weeklyNotice` on, [`pet_available`], and this
/// week's Monday not yet in `diary.lastNoticeWeek`. An empty last week is marked as handled here
/// (`settings::put`) and offers nothing.
fn diary_due(conn: &Connection, now: DateTime<Local>) -> AppResult<Option<DiaryReady>> {
    if !settings::get::<bool>(conn, "diary.weeklyNotice")? || !pet_available(conn, now)? {
        return Ok(None);
    }
    let now_ms = now.timestamp_millis();
    let week = diary::week_bounds(&Local, now_ms, 0)?.start.format("%Y-%m-%d").to_string();
    if settings::get::<Option<String>>(conn, "diary.lastNoticeWeek")?.as_deref() == Some(week.as_str()) {
        return Ok(None);
    }
    if diary::is_empty(&diary::week(conn, &Local, now_ms, -1)?) {
        settings::put(conn, "diary.lastNoticeWeek", &Value::String(week))?;
        return Ok(None);
    }
    Ok(Some(DiaryReady { week }))
}

/// [`diary_due`] while the user is at the Mac (`idle::seconds_since_input() < RETURN_SECS`), outside
/// sleep hours, not hidden for full screen, the pet awake (`Sleep`) and nothing recording
/// (`Recorder::is_active`). The pet records the week when it shows the notice, so one it couldn't
/// show is offered again next tick.
fn offer_diary(app: &AppHandle) {
    if idle::seconds_since_input() >= RETURN_SECS
        || idle::in_sleep_hours(app)
        || app.state::<Visibility>().current().hidden
        || app.state::<Sleep>().current()
        || app.try_state::<Recorder>().is_some_and(|r| r.is_active())
    {
        return;
    }
    let due = diary_due(&app.state::<Db>().conn(), Local::now());
    match due {
        Ok(Some(ev)) => events::diary_ready(app, ev),
        Ok(None) => {}
        Err(e) => eprintln!("[next] diary notice: {e}"),
    }
}

/// Every guardrail: [`settings_allow`], outside sleep hours, and not hidden for full screen.
fn allowed(app: &AppHandle, trigger: &str) -> AppResult<bool> {
    let ok = settings_allow(&app.state::<Db>().conn(), trigger, Local::now())?;
    Ok(ok && !idle::in_sleep_hours(app) && !app.state::<Visibility>().current().hidden)
}

/// Sends the card for `trigger` to the pet when every guardrail passes and a task is eligible.
async fn propose(app: &AppHandle, trigger: &'static str) {
    match allowed(app, trigger) {
        Ok(true) => {}
        Ok(false) => return,
        Err(e) => return eprintln!("[next] {trigger} guardrails: {e}"),
    }
    match super::pick(app, trigger).await {
        Ok(Some(card)) => events::whats_next(app, WhatsNextEvent { card, trigger: trigger.to_owned() }),
        Ok(None) => {}
        Err(e) => eprintln!("[next] {trigger} pick failed: {e}"),
    }
}

/// Seconds away before a return counts, or `None` while `whatsNext.onReturn` is off.
fn away_secs(conn: &Connection) -> AppResult<Option<f64>> {
    if !settings::get::<bool>(conn, "whatsNext.onReturn")? {
        return Ok(None);
    }
    Ok(Some(settings::get::<f64>(conn, "whatsNext.idleMinutes")? * 60.0))
}

/// The 30 s scheduler. Each tick: emit `counts-changed` when the due-soon flag flips, offer last
/// week's diary notice, then (while `whatsNext.onReturn` is on) run return detection and offer the
/// `return` card.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(TICK);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut due_soon: Option<bool> = None;
        let mut watch = ReturnWatch::default();
        loop {
            interval.tick().await;
            let (soon, away) = {
                let conn = app.state::<Db>().inner().conn();
                (tasks::counts(&conn, now_ms()).map(|c| c.2), away_secs(&conn))
            };
            match soon {
                Ok(soon) => {
                    if due_soon.is_some_and(|last| last != soon) {
                        items::emit_counts(&app);
                    }
                    due_soon = Some(soon);
                }
                Err(e) => eprintln!("[next] due-soon check: {e}"),
            }
            offer_diary(&app);
            let away = match away {
                Ok(Some(secs)) => secs,
                Ok(None) => {
                    watch = ReturnWatch::default();
                    continue;
                }
                Err(e) => {
                    eprintln!("[next] return settings: {e}");
                    continue;
                }
            };
            if watch.tick(idle::seconds_since_input(), away) {
                if app.state::<Sleep>().current() {
                    tokio::time::sleep(WAKE_GRACE).await;
                }
                propose(&app, "return").await;
            }
        }
    });
}

/// After a task is marked done from Home (or by an agent): the next card 2 s later.
pub fn after_done(app: &AppHandle) {
    offer_later(app, "afterDone");
}

/// After a meeting's first summary created tasks: the next card 2 s later.
pub fn after_meeting(app: &AppHandle) {
    offer_later(app, "afterMeeting");
}

fn offer_later(app: &AppHandle, trigger: &'static str) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(AFTER_DONE_DELAY).await;
        propose(&app, trigger).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rusqlite::params;
    use serde_json::json;

    use crate::db::new_id;

    fn hours(start: &str, end: &str, days: &[u32]) -> WorkHours {
        WorkHours { start: start.into(), end: end.into(), days: days.to_vec() }
    }

    /// 2026-10-16 is a Friday.
    fn at(day: u32, h: u32, m: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 10, day, h, m, 0).unwrap()
    }

    #[test]
    fn work_hours_window_and_days() {
        let h = hours("09:00", "18:00", &[1, 2, 3, 4, 5]);
        assert!(!in_work_hours(&h, at(16, 8, 59).naive_local()));
        assert!(in_work_hours(&h, at(16, 9, 0).naive_local()));
        assert!(in_work_hours(&h, at(16, 17, 59).naive_local()));
        assert!(!in_work_hours(&h, at(16, 18, 0).naive_local()));
        // Saturday 17 and Sunday 18 Oct.
        assert!(!in_work_hours(&h, at(17, 10, 0).naive_local()));
        assert!(in_work_hours(&hours("09:00", "18:00", &[7]), at(18, 10, 0).naive_local()));
        assert!(!in_work_hours(&hours("9am", "18:00", &[5]), at(16, 10, 0).naive_local()));
    }

    #[test]
    fn return_fires_once_after_being_away() {
        let mut w = ReturnWatch::default();
        assert!(!w.tick(5.0, 600.0), "never away: no return");
        assert!(!w.tick(599.0, 600.0));
        assert!(!w.tick(600.0, 600.0));
        assert!(w.away);
        assert!(!w.tick(45.0, 600.0), "input 45 s ago: not yet back");
        assert!(w.away);
        assert!(w.tick(2.0, 600.0));
        assert!(!w.away);
        assert!(!w.tick(1.0, 600.0), "fires once per return");
    }

    fn task(conn: &Connection) -> String {
        let id = new_id();
        conn.execute(
            "INSERT INTO tasks (id, title, created_at, updated_at) VALUES (?1, 'T', 0, 0)",
            [&id],
        )
        .unwrap();
        id
    }

    fn feedback(conn: &Connection, task_id: &str, trigger: &str, action: &str, at: i64) {
        conn.execute(
            "INSERT INTO next_feedback (id, task_id, trigger, action, at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![new_id(), task_id, trigger, action, at],
        )
        .unwrap();
    }

    #[test]
    fn shown_counts_only_proactive_cards_in_the_last_hour() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        let t = task(&conn);
        let now = at(16, 10, 0).timestamp_millis();
        feedback(&conn, &t, "return", "shown", now - 60_000);
        feedback(&conn, &t, "afterDone", "shown", now - HOUR_MS + 1);
        feedback(&conn, &t, "afterDone", "shown", now - HOUR_MS); // exactly an hour ago: out
        feedback(&conn, &t, "bubble", "shown", now - 60_000);
        feedback(&conn, &t, "return", "skip", now - 60_000);
        assert_eq!(shown_last_hour(&conn, now).unwrap(), 2);
    }

    #[test]
    fn settings_guardrails() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        let now = at(16, 10, 0);
        let set = |k: &str, v: serde_json::Value| settings::put(&conn, k, &v).unwrap();
        assert!(!settings_allow(&conn, "return", now).unwrap(), "onboarding not completed");
        set("onboarding.completed", json!(true));
        assert!(settings_allow(&conn, "return", now).unwrap());
        assert!(settings_allow(&conn, "afterDone", now).unwrap());
        assert!(matches!(settings_allow(&conn, "bubble", now), Err(AppError::Code("invalid"))));

        set("whatsNext.onReturn", json!(false));
        assert!(!settings_allow(&conn, "return", now).unwrap());
        assert!(settings_allow(&conn, "afterDone", now).unwrap());
        set("whatsNext.afterDone", json!(false));
        assert!(!settings_allow(&conn, "afterDone", now).unwrap());
        set("whatsNext.afterDone", json!(true));

        assert!(!settings_allow(&conn, "afterDone", at(16, 19, 0)).unwrap(), "after work hours");
        assert!(!settings_allow(&conn, "afterDone", at(17, 10, 0)).unwrap(), "Saturday");

        set("pet.paused", json!(true));
        assert!(!settings_allow(&conn, "afterDone", now).unwrap());
        set("pet.paused", json!(false));

        let ms = now.timestamp_millis();
        set("pet.hiddenUntil", json!(ms + 60_000));
        assert!(!settings_allow(&conn, "afterDone", now).unwrap());
        set("pet.hiddenUntil", json!(ms - 60_000));
        assert!(settings_allow(&conn, "afterDone", now).unwrap(), "hide has ended");

        let t = task(&conn);
        feedback(&conn, &t, "return", "shown", ms - 120_000);
        assert!(settings_allow(&conn, "afterDone", now).unwrap(), "1 of 2 per hour");
        feedback(&conn, &t, "afterDone", "shown", ms - 60_000);
        assert!(!settings_allow(&conn, "afterDone", now).unwrap(), "2 of 2 per hour");
        set("whatsNext.maxPerHour", json!(3));
        assert!(settings_allow(&conn, "afterDone", now).unwrap());
    }

    #[test]
    fn after_meeting_trigger_respects_switch() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        let now = at(16, 10, 0);
        let set = |k: &str, v: serde_json::Value| settings::put(&conn, k, &v).unwrap();
        set("onboarding.completed", json!(true));
        assert!(settings_allow(&conn, "afterMeeting", now).unwrap(), "on by default");
        set("whatsNext.afterMeeting", json!(false));
        assert!(!settings_allow(&conn, "afterMeeting", now).unwrap());
        assert!(settings_allow(&conn, "afterDone", now).unwrap(), "other switches unaffected");
        set("whatsNext.afterMeeting", json!(true));
        let ms = now.timestamp_millis();
        let t = task(&conn);
        feedback(&conn, &t, "afterMeeting", "shown", ms - 60_000);
        feedback(&conn, &t, "return", "shown", ms - 60_000);
        assert_eq!(shown_last_hour(&conn, ms).unwrap(), 2);
        assert!(!settings_allow(&conn, "afterMeeting", now).unwrap(), "counts toward 2 per hour");
    }

    fn item(conn: &Connection, at: i64) {
        conn.execute(
            "INSERT INTO items (id, kind, title, source_kind, content_hash, status, captured_at, updated_at)
             VALUES (?1, 'text', ?1, 'drop', ?1, 'inbox', ?2, ?2)",
            params![new_id(), at],
        )
        .unwrap();
    }

    #[test]
    fn diary_notice_once_per_week() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        let set = |k: &str, v: serde_json::Value| settings::put(&conn, k, &v).unwrap();
        set("onboarding.completed", json!(true));
        item(&conn, at(14, 10, 0).timestamp_millis());
        let now = at(19, 10, 0);
        let due = || diary_due(&conn, now).unwrap().map(|d| d.week);
        assert_eq!(due(), Some("2026-10-19".to_owned()));
        set("diary.lastNoticeWeek", json!("2026-10-19"));
        assert_eq!(due(), None, "already shown this week");
        set("diary.lastNoticeWeek", json!("2026-10-12"));
        assert_eq!(due(), Some("2026-10-19".to_owned()));

        set("diary.weeklyNotice", json!(false));
        assert_eq!(due(), None, "switch off");
        set("diary.weeklyNotice", json!(true));
        set("pet.paused", json!(true));
        assert_eq!(due(), None, "paused");
        set("pet.paused", json!(false));
        assert!(diary_due(&conn, at(24, 10, 0)).unwrap().is_none(), "Saturday");
    }

    #[test]
    fn empty_last_week_marks_without_notice() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        settings::put(&conn, "onboarding.completed", &json!(true)).unwrap();
        assert!(diary_due(&conn, at(19, 10, 0)).unwrap().is_none());
        assert_eq!(
            settings::get::<Option<String>>(&conn, "diary.lastNoticeWeek").unwrap().as_deref(),
            Some("2026-10-19")
        );
    }
}
