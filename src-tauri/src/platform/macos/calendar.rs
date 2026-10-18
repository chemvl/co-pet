//! EventKit half of `platform::calendar`.
use objc2::rc::{autoreleasepool, Retained};
use objc2_event_kit::{
    EKCalendar, EKEntityType, EKEventAvailability, EKEventStatus, EKEventStore, EKParticipantStatus,
};
use objc2_foundation::{NSArray, NSDate};

use super::permissions;
use crate::model::CalendarInfo;
use crate::platform::calendar::{counts_as_busy, Availability, Busy, EventFacts};

/// EventKit's availability; values EventKit may add later count like `NotSupported` (busy).
fn availability(a: EKEventAvailability) -> Availability {
    if a == EKEventAvailability::Busy {
        Availability::Busy
    } else if a == EKEventAvailability::Free {
        Availability::Free
    } else if a == EKEventAvailability::Tentative {
        Availability::Tentative
    } else if a == EKEventAvailability::Unavailable {
        Availability::Unavailable
    } else {
        Availability::NotSupported
    }
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
                        availability: availability(ev.availability()),
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
