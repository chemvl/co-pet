//! The pet diary: per-week counts computed live from synced rows (items, tasks, meetings), so
//! every Mac shows the same page and Trash or Undo apply at once. Nothing is stored.
use chrono::{Datelike, NaiveDate, NaiveTime, TimeDelta, TimeZone};
use rusqlite::Connection;

use crate::error::{AppError, AppResult};
use crate::model::{DiaryCategory, DiaryDay, DiaryWeek};

/// Oldest week `diary_week` serves (10 years back).
pub const MAX_WEEKS_BACK: i64 = 520;
/// Saved items: live, not in the trash, not a meeting's own item (the meeting counts instead).
const SAVED_ITEMS: &str = "deleted_at IS NULL AND status <> 'trashed' AND meeting_id IS NULL";
const DONE_TASKS: &str = "deleted_at IS NULL AND status = 'done'";
const DONE_MEETINGS: &str = "deleted_at IS NULL AND state = 'done'";

/// A local week: `bounds[i]` = epoch ms of local midnight of day `i` (0 = Monday); `bounds[7]` =
/// the next Monday.
#[derive(Debug, Clone, PartialEq)]
pub struct Week {
    pub start: NaiveDate,
    pub bounds: [i64; 8],
}

/// Epoch ms of `date` 00:00 in `tz` (the earlier instant when ambiguous). When midnight is
/// skipped by DST, 01:00; when that is skipped too, `AppError::msg("no local midnight")`.
pub fn local_midnight<Tz: TimeZone>(tz: &Tz, date: NaiveDate) -> AppResult<i64> {
    let at = |t: NaiveTime| tz.from_local_datetime(&date.and_time(t)).earliest();
    at(NaiveTime::MIN)
        .or_else(|| NaiveTime::from_hms_opt(1, 0, 0).and_then(at))
        .map(|d| d.timestamp_millis())
        .ok_or_else(|| AppError::msg("no local midnight"))
}

/// The week `offset` weeks from the one holding `now_ms` in `tz` (Monday from
/// `date_naive() - weekday().num_days_from_monday()`, then `checked_add_signed(TimeDelta::weeks(offset))`).
pub fn week_bounds<Tz: TimeZone>(tz: &Tz, now_ms: i64, offset: i64) -> AppResult<Week> {
    let now = tz.timestamp_millis_opt(now_ms).single().ok_or(AppError::Code("invalid"))?;
    let today = now.date_naive();
    let start = today
        .checked_sub_signed(TimeDelta::days(i64::from(today.weekday().num_days_from_monday())))
        .and_then(|monday| monday.checked_add_signed(TimeDelta::weeks(offset)))
        .ok_or(AppError::Code("invalid"))?;
    let mut bounds = [0i64; 8];
    for (i, b) in bounds.iter_mut().enumerate() {
        let day = start
            .checked_add_signed(TimeDelta::days(i as i64))
            .ok_or(AppError::Code("invalid"))?;
        *b = local_midnight(tz, day)?;
    }
    Ok(Week { start, bounds })
}

/// Day 0–6 of `at` (callers pass `bounds[0] <= at < bounds[7]`).
fn day_index(bounds: &[i64; 8], at: i64) -> usize {
    bounds[1..7].partition_point(|b| *b <= at)
}

fn ymd(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// Timestamps (and a second value, 0 when absent) returned by `sql` for `[from, to)`.
fn rows(conn: &Connection, sql: &str, from: i64, to: i64) -> AppResult<Vec<(i64, i64)>> {
    let mut stmt = conn.prepare(sql)?;
    let two = stmt.column_count() > 1;
    let out = stmt
        .query_map([from, to], |r| Ok((r.get(0)?, if two { r.get(1)? } else { 0 })))?
        .collect::<Result<_, _>>()?;
    Ok(out)
}

/// `offset > 0` or `< -MAX_WEEKS_BACK` → `AppError::Code("invalid")`.
pub fn week<Tz: TimeZone>(conn: &Connection, tz: &Tz, now_ms: i64, offset: i64) -> AppResult<DiaryWeek> {
    if offset > 0 || offset < -MAX_WEEKS_BACK {
        return Err(AppError::Code("invalid"));
    }
    let w = week_bounds(tz, now_ms, offset)?;
    let (from, to) = (w.bounds[0], w.bounds[7]);
    let mut days = (0..7)
        .map(|i| {
            Ok(DiaryDay {
                date: ymd(w.start.checked_add_signed(TimeDelta::days(i)).ok_or(AppError::Code("invalid"))?),
                captured: 0,
                tasks_done: 0,
                meetings: 0,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    let items = format!("SELECT captured_at FROM items WHERE {SAVED_ITEMS} AND captured_at >= ?1 AND captured_at < ?2");
    for (at, _) in rows(conn, &items, from, to)? {
        days[day_index(&w.bounds, at)].captured += 1;
    }
    let tasks = format!("SELECT done_at FROM tasks WHERE {DONE_TASKS} AND done_at >= ?1 AND done_at < ?2");
    for (at, _) in rows(conn, &tasks, from, to)? {
        days[day_index(&w.bounds, at)].tasks_done += 1;
    }
    let meetings = format!(
        "SELECT started_at, duration_ms FROM meetings WHERE {DONE_MEETINGS} AND started_at >= ?1 AND started_at < ?2"
    );
    let mut meeting_ms = 0;
    for (at, ms) in rows(conn, &meetings, from, to)? {
        days[day_index(&w.bounds, at)].meetings += 1;
        meeting_ms += ms;
    }

    let mut stmt = conn.prepare(
        "SELECT i.category_id, COUNT(*) AS n FROM items i JOIN categories c ON c.id = i.category_id AND c.deleted_at IS NULL WHERE i.deleted_at IS NULL AND i.status <> 'trashed' AND i.meeting_id IS NULL AND i.captured_at >= ?1 AND i.captured_at < ?2 GROUP BY i.category_id ORDER BY n DESC, c.position, i.category_id LIMIT 3",
    )?;
    let top_categories = stmt
        .query_map([from, to], |r| Ok(DiaryCategory { category_id: r.get(0)?, count: r.get(1)? }))?
        .collect::<Result<Vec<_>, _>>()?;

    let has_previous: bool = conn.query_row(
        &format!(
            "SELECT EXISTS (SELECT 1 FROM items WHERE {SAVED_ITEMS} AND captured_at < ?1) OR EXISTS (SELECT 1 FROM tasks WHERE {DONE_TASKS} AND done_at < ?1) OR EXISTS (SELECT 1 FROM meetings WHERE {DONE_MEETINGS} AND started_at < ?1)"
        ),
        [from],
        |r| r.get(0),
    )?;

    let end = w.start.checked_add_signed(TimeDelta::days(6)).ok_or(AppError::Code("invalid"))?;
    Ok(DiaryWeek {
        offset,
        start: ymd(w.start),
        end: ymd(end),
        captured: days.iter().map(|d| d.captured).sum(),
        tasks_done: days.iter().map(|d| d.tasks_done).sum(),
        meetings: days.iter().map(|d| d.meetings).sum(),
        meeting_ms,
        top_categories,
        days,
        has_previous,
    })
}

/// Last week, or this week when last week is empty: the page the diary opens on.
pub fn latest<Tz: TimeZone>(conn: &Connection, tz: &Tz, now_ms: i64) -> AppResult<DiaryWeek> {
    let last = week(conn, tz, now_ms, -1)?;
    if is_empty(&last) {
        week(conn, tz, now_ms, 0)
    } else {
        Ok(last)
    }
}

/// Nothing saved, done or recorded.
pub fn is_empty(w: &DiaryWeek) -> bool {
    w.captured == 0 && w.tasks_done == 0 && w.meetings == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{now_ms, tasks, Db};
    use crate::model::{Actor, NewTask};
    use chrono::{FixedOffset, Local};
    use chrono_tz::Europe::Berlin;
    use rusqlite::params;

    const H: i64 = 3_600_000;

    fn at<Tz: TimeZone>(tz: &Tz, y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> i64 {
        tz.with_ymd_and_hms(y, m, d, h, min, s).earliest().unwrap().timestamp_millis()
    }

    fn plus7() -> FixedOffset {
        FixedOffset::east_opt(7 * 3600).unwrap()
    }

    fn item(conn: &Connection, id: &str, status: &str, cat: &str, at: i64, deleted: Option<i64>, meeting: Option<&str>) {
        conn.execute(
            "INSERT INTO items (id, kind, title, source_kind, content_hash, category_id, status, meeting_id,
               captured_at, updated_at, deleted_at)
             VALUES (?1, 'text', ?1, 'drop', ?1, ?2, ?3, ?4, ?5, ?5, ?6)",
            params![id, cat, status, meeting, at, deleted],
        )
        .unwrap();
    }

    fn task(conn: &Connection, id: &str, status: &str, done_at: Option<i64>, deleted: Option<i64>) {
        conn.execute(
            "INSERT INTO tasks (id, title, status, done_at, created_at, updated_at, deleted_at)
             VALUES (?1, ?1, ?2, ?3, 1, 1, ?4)",
            params![id, status, done_at, deleted],
        )
        .unwrap();
    }

    fn meeting(conn: &Connection, id: &str, state: &str, started: i64, ms: i64, deleted: Option<i64>) {
        conn.execute(
            "INSERT INTO meetings (id, title, source, device_id, started_at, duration_ms, state,
               created_at, updated_at, deleted_at)
             VALUES (?1, ?1, 'recording', 'dev', ?2, ?3, ?4, 1, 1, ?5)",
            params![id, started, ms, state, deleted],
        )
        .unwrap();
    }

    #[test]
    fn week_starts_on_local_monday() {
        let sun = at(&Berlin, 2026, 10, 18, 23, 30, 0);
        assert_eq!(week_bounds(&Berlin, sun, 0).unwrap().start, NaiveDate::from_ymd_opt(2026, 10, 12).unwrap());
        let mon = at(&Berlin, 2026, 10, 19, 0, 0, 0);
        assert_eq!(week_bounds(&Berlin, mon, 0).unwrap().start, NaiveDate::from_ymd_opt(2026, 10, 19).unwrap());
        assert_eq!(week_bounds(&Berlin, mon, -1).unwrap().start, NaiveDate::from_ymd_opt(2026, 10, 12).unwrap());
    }

    #[test]
    fn dst_fall_back_week_is_169_hours() {
        let w = week_bounds(&Berlin, at(&Berlin, 2026, 10, 21, 12, 0, 0), 0).unwrap();
        assert_eq!(w.bounds[7] - w.bounds[0], 169 * H);
        assert_eq!(w.bounds[7] - w.bounds[6], 25 * H);
        assert_eq!(day_index(&w.bounds, at(&Berlin, 2026, 10, 25, 23, 30, 0)), 6);
    }

    #[test]
    fn dst_spring_forward_week_is_167_hours() {
        let w = week_bounds(&Berlin, at(&Berlin, 2026, 3, 25, 12, 0, 0), 0).unwrap();
        assert_eq!(w.bounds[7] - w.bounds[0], 167 * H);
    }

    #[test]
    fn offset_out_of_range_is_invalid() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        for offset in [1, -MAX_WEEKS_BACK - 1] {
            assert!(matches!(week(&conn, &Local, now_ms(), offset), Err(AppError::Code("invalid"))));
        }
    }

    #[test]
    fn week_counts_live_rows_only() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        let tz = plus7();
        let t = |d: u32, h: u32, m: u32, s: u32| at(&tz, 2026, 10, d, h, m, s);
        item(&conn, "A", "inbox", "code", t(12, 8, 0, 0), None, None);
        item(&conn, "B", "archived", "code", t(14, 10, 0, 0), None, None);
        item(&conn, "C", "trashed", "code", t(14, 11, 0, 0), None, None);
        item(&conn, "D", "inbox", "code", t(14, 12, 0, 0), Some(t(14, 13, 0, 0)), None);
        item(&conn, "E", "inbox", "code", t(14, 14, 0, 0), None, Some("m1"));
        item(&conn, "F", "inbox", "code", t(11, 23, 59, 0), None, None);
        item(&conn, "G", "inbox", "read", t(18, 23, 59, 59), None, None);
        task(&conn, "T1", "done", Some(t(13, 9, 0, 0)), None);
        task(&conn, "T2", "open", None, None);
        task(&conn, "T3", "done", Some(t(13, 10, 0, 0)), Some(t(13, 11, 0, 0)));
        task(&conn, "T4", "done", Some(t(19, 0, 0, 0)), None);
        meeting(&conn, "M1", "done", t(15, 14, 0, 0), 1_800_000, None);
        meeting(&conn, "M2", "failed", t(15, 15, 0, 0), 600_000, None);
        meeting(&conn, "M3", "done", t(15, 16, 0, 0), 600_000, Some(t(15, 17, 0, 0)));

        let w = week(&conn, &tz, t(16, 12, 0, 0), 0).unwrap();
        assert_eq!((w.captured, w.tasks_done, w.meetings, w.meeting_ms), (3, 1, 1, 1_800_000));
        assert_eq!(
            w.top_categories,
            vec![
                DiaryCategory { category_id: "code".into(), count: 2 },
                DiaryCategory { category_id: "read".into(), count: 1 },
            ]
        );
        assert_eq!(w.days.len(), 7);
        assert_eq!(w.days[0].captured, 1);
        assert_eq!(w.days[2].captured, 1);
        assert_eq!(w.days[6].captured, 1);
        assert_eq!(w.days[1].tasks_done, 1);
        assert_eq!(w.days[3].meetings, 1);
        assert_eq!(w.days[0].date, "2026-10-12");
        assert_eq!(w.days[6].date, "2026-10-18");
        assert_eq!((w.start.as_str(), w.end.as_str()), ("2026-10-12", "2026-10-18"));
        assert!(w.has_previous);
    }

    #[test]
    fn undone_done_is_not_counted() {
        let db = Db::open_in_memory().unwrap();
        let mut conn = db.conn();
        let new = NewTask { title: "Write report".into(), ..Default::default() };
        let id = tasks::create(&mut conn, &new, &Actor::User).unwrap().0.id;
        let ev = tasks::set_done(&mut conn, &id, true, &Actor::User).unwrap();
        assert_eq!(week(&conn, &Local, now_ms(), 0).unwrap().tasks_done, 1);
        tasks::undo_event(&mut conn, &ev[0]).unwrap();
        assert_eq!(week(&conn, &Local, now_ms(), 0).unwrap().tasks_done, 0);
        tasks::set_done(&mut conn, &id, true, &Actor::User).unwrap();
        tasks::set_done(&mut conn, &id, false, &Actor::User).unwrap();
        assert_eq!(week(&conn, &Local, now_ms(), 0).unwrap().tasks_done, 0);
    }

    #[test]
    fn latest_falls_back_to_this_week() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        let tz = plus7();
        let now = at(&tz, 2026, 10, 16, 12, 0, 0);
        item(&conn, "this", "inbox", "code", at(&tz, 2026, 10, 13, 9, 0, 0), None, None);
        assert_eq!(latest(&conn, &tz, now).unwrap().offset, 0);
        item(&conn, "last", "inbox", "code", at(&tz, 2026, 10, 7, 9, 0, 0), None, None);
        assert_eq!(latest(&conn, &tz, now).unwrap().offset, -1);
    }

    #[test]
    fn has_previous_only_with_earlier_activity() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        let tz = plus7();
        let now = at(&tz, 2026, 10, 16, 12, 0, 0);
        assert!(!week(&conn, &tz, now, 0).unwrap().has_previous);
        let three_weeks_ago = at(&tz, 2026, 9, 23, 9, 0, 0);
        item(&conn, "old-trashed", "trashed", "code", three_weeks_ago, None, None);
        assert!(!week(&conn, &tz, now, 0).unwrap().has_previous);
        item(&conn, "old-live", "inbox", "code", three_weeks_ago, None, None);
        assert!(week(&conn, &tz, now, 0).unwrap().has_previous);
    }
}
