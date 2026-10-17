//! EventKit reads for What's Next: the event calendars and the busy times in a window. Nothing read here is stored, synced or logged, and event titles are never read. Blocking: call from spawn_blocking.
use objc2::rc::{autoreleasepool, Retained};
use objc2_event_kit::{
    EKCalendar, EKEntityType, EKEventAvailability, EKEventStatus, EKEventStore, EKParticipantStatus,
};
use objc2_foundation::{NSArray, NSDate};

use super::permissions;
use crate::model::CalendarInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Busy {
    pub start_ms: i64,
    pub end_ms: i64,
}

/// What decides whether one event blocks time.
#[derive(Debug, Clone, Copy)]
pub struct EventFacts {
    pub all_day: bool,
    pub availability: EKEventAvailability,
    pub canceled: bool,
    pub declined: bool,
}

/// Timed, not marked free, not canceled, not declined by the user.
pub fn counts_as_busy(e: &EventFacts) -> bool {
    !e.all_day && e.availability != EKEventAvailability::Free && !e.canceled && !e.declined
}

/// Event calendars on this Mac sorted by account then title; `[]` without full access.
pub fn calendars() -> Vec<CalendarInfo> {
    if permissions::calendar_status() != "granted" {
        return Vec::new();
    }
    autoreleasepool(|_| {
        // SAFETY: plain `EKEventStore` init and getters, used on this thread only.
        let mut out: Vec<CalendarInfo> = unsafe {
            let store = EKEventStore::new();
            store
                .calendarsForEntityType(EKEntityType::Event)
                .iter()
                .map(|c| CalendarInfo {
                    id: c.calendarIdentifier().to_string(),
                    title: c.title().to_string(),
                    account: c.source().map(|s| s.title().to_string()).unwrap_or_default(),
                })
                .collect()
        };
        out.sort_by(|a, b| (&a.account, &a.title).cmp(&(&b.account, &b.title)));
        out
    })
}

/// Busy times overlapping `[from_ms, to_ms)` in every event calendar not in `excluded`, sorted
/// by start; `[]` without full access or with every calendar excluded.
#[allow(dead_code)] // read by `next::slot::current` (Task 5.4)
pub fn busy_between(from_ms: i64, to_ms: i64, excluded: &[String]) -> Vec<Busy> {
    if permissions::calendar_status() != "granted" {
        return Vec::new();
    }
    autoreleasepool(|_| {
        // SAFETY: plain `EKEventStore` init, getters and a predicate built by the same store,
        // used on this thread only.
        let mut out: Vec<Busy> = unsafe {
            let store = EKEventStore::new();
            let cals: Vec<Retained<EKCalendar>> = store
                .calendarsForEntityType(EKEntityType::Event)
                .iter()
                .filter(|c| {
                    let id = c.calendarIdentifier().to_string();
                    !excluded.iter().any(|x| *x == id)
                })
                .collect();
            // `None` would mean every calendar.
            if cals.is_empty() {
                return Vec::new();
            }
            let cals = NSArray::from_retained_slice(&cals);
            let start = NSDate::dateWithTimeIntervalSince1970(from_ms as f64 / 1000.0);
            let end = NSDate::dateWithTimeIntervalSince1970(to_ms as f64 / 1000.0);
            let pred = store.predicateForEventsWithStartDate_endDate_calendars(&start, &end, Some(&cals));
            store
                .eventsMatchingPredicate(&pred)
                .iter()
                .filter(|ev| {
                    counts_as_busy(&EventFacts {
                        all_day: ev.isAllDay(),
                        availability: ev.availability(),
                        canceled: ev.status() == EKEventStatus::Canceled,
                        declined: ev.attendees().is_some_and(|a| {
                            a.iter().any(|p| p.isCurrentUser() && p.participantStatus() == EKParticipantStatus::Declined)
                        }),
                    })
                })
                .map(|ev| Busy {
                    start_ms: (ev.startDate().timeIntervalSince1970() * 1000.0) as i64,
                    end_ms: (ev.endDate().timeIntervalSince1970() * 1000.0) as i64,
                })
                .collect()
        };
        out.sort_by_key(|b| b.start_ms);
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_rule_skips_all_day_free_canceled_declined() {
        let base = EventFacts { all_day: false, availability: EKEventAvailability::Busy, canceled: false, declined: false };
        assert!(counts_as_busy(&base));
        for availability in [EKEventAvailability::Tentative, EKEventAvailability::Unavailable, EKEventAvailability::NotSupported] {
            assert!(counts_as_busy(&EventFacts { availability, ..base }));
        }
        assert!(!counts_as_busy(&EventFacts { all_day: true, ..base }));
        assert!(!counts_as_busy(&EventFacts { availability: EKEventAvailability::Free, ..base }));
        assert!(!counts_as_busy(&EventFacts { canceled: true, ..base }));
        assert!(!counts_as_busy(&EventFacts { declined: true, ..base }));
    }
}
