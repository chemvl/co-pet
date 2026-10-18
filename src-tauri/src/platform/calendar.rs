//! Calendar reads for What's Next: the event calendars and the busy times in a window. Nothing read
//! here is stored, synced or logged, and event titles are never read. Blocking: call from
//! spawn_blocking.
pub use super::os::calendar::{busy_between, calendars};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Busy {
    pub start_ms: i64,
    pub end_ms: i64,
}

/// How an event shows the user's time (EventKit's `EKEventAvailability`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Busy,
    Free,
    Tentative,
    Unavailable,
    NotSupported,
}

/// What decides whether one event blocks time.
#[derive(Debug, Clone, Copy)]
pub struct EventFacts {
    pub all_day: bool,
    pub availability: Availability,
    pub canceled: bool,
    pub declined: bool,
}

/// Timed, not marked free, not canceled, not declined by the user.
pub fn counts_as_busy(e: &EventFacts) -> bool {
    !e.all_day && e.availability != Availability::Free && !e.canceled && !e.declined
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_rule_skips_all_day_free_canceled_declined() {
        let base = EventFacts { all_day: false, availability: Availability::Busy, canceled: false, declined: false };
        assert!(counts_as_busy(&base));
        for availability in [Availability::Tentative, Availability::Unavailable, Availability::NotSupported] {
            assert!(counts_as_busy(&EventFacts { availability, ..base }));
        }
        assert!(!counts_as_busy(&EventFacts { all_day: true, ..base }));
        assert!(!counts_as_busy(&EventFacts { availability: Availability::Free, ..base }));
        assert!(!counts_as_busy(&EventFacts { canceled: true, ..base }));
        assert!(!counts_as_busy(&EventFacts { declined: true, ..base }));
    }
}
