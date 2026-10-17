//! The free time before the next calendar event, for one What's Next pick. In memory only.
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::db::{settings, Db};
use crate::platform::macos::calendar::{self, Busy};
use crate::platform::macos::permissions;

const LOOKAHEAD_MS: i64 = 12 * 3_600_000;
const READ_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    /// An event is running now.
    pub busy_now: bool,
    /// Whole minutes until the next event; `None` while busy or with no event in 12 h.
    pub free_min: Option<i64>,
    /// Start of that next event (epoch ms).
    pub next_at: Option<i64>,
}

pub fn slot_at(busy: &[Busy], now_ms: i64) -> Slot {
    if busy.iter().any(|b| b.start_ms <= now_ms && now_ms < b.end_ms) {
        return Slot { busy_now: true, free_min: None, next_at: None };
    }
    let next_at = busy.iter().map(|b| b.start_ms).filter(|&s| s > now_ms).min();
    Slot { busy_now: false, free_min: next_at.map(|at| (at - now_ms) / 60_000), next_at }
}

/// `None` when `whatsNext.calendar` is off, access isn't granted, or EventKit fails or takes
/// over 2 s (logged; the pick goes on without the calendar).
pub async fn current(app: &AppHandle, now_ms: i64) -> Option<Slot> {
    let read = {
        let db = app.state::<Db>();
        let conn = db.conn();
        settings::get::<bool>(&conn, "whatsNext.calendar")
            .and_then(|on| Ok((on, settings::get::<Vec<String>>(&conn, "whatsNext.calendarExcluded")?)))
    };
    let excluded = match read {
        Ok((true, excluded)) => excluded,
        Ok((false, _)) => return None,
        Err(e) => {
            eprintln!("[next] calendar settings: {e}");
            return None;
        }
    };
    if permissions::calendar_status() != "granted" {
        return None;
    }
    let task = tauri::async_runtime::spawn_blocking(move || {
        calendar::busy_between(now_ms, now_ms + LOOKAHEAD_MS, &excluded)
    });
    match tokio::time::timeout(READ_TIMEOUT, task).await {
        Ok(Ok(busy)) => Some(slot_at(&busy, now_ms)),
        Ok(Err(e)) => {
            eprintln!("[next] calendar read: {e}");
            None
        }
        Err(_) => {
            eprintln!("[next] calendar read: timed out");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};

    /// Fri 16 Oct 2026 at `h:m` local, epoch ms.
    fn at(h: u32, m: u32) -> i64 {
        Local.with_ymd_and_hms(2026, 10, 16, h, m, 0).unwrap().timestamp_millis()
    }

    fn busy(from: (u32, u32), to: (u32, u32)) -> Busy {
        Busy { start_ms: at(from.0, from.1), end_ms: at(to.0, to.1) }
    }

    #[test]
    fn slot_is_minutes_until_next_busy() {
        let s = slot_at(&[busy((15, 0), (16, 0)), busy((14, 0), (14, 30))], at(13, 40));
        assert_eq!(s, Slot { busy_now: false, free_min: Some(20), next_at: Some(at(14, 0)) });
    }

    #[test]
    fn slot_inside_event_is_busy_now() {
        let s = slot_at(&[busy((13, 30), (14, 0))], at(13, 40));
        assert!(s.busy_now);
        assert_eq!(s.free_min, None);
    }

    #[test]
    fn slot_ignores_ended_events() {
        let s = slot_at(&[busy((13, 0), (13, 40))], at(13, 40));
        assert_eq!(s, Slot { busy_now: false, free_min: None, next_at: None });
    }

    #[test]
    fn slot_without_events_is_open() {
        assert_eq!(slot_at(&[], at(13, 40)), Slot { busy_now: false, free_min: None, next_at: None });
    }
}
