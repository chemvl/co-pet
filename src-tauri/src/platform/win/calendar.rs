//! D10: Windows has no calendar source for an unpackaged app (AppointmentStore needs package
//! identity). What's Next runs without busy times; the UI hides the calendar block.
use crate::model::CalendarInfo;
use crate::platform::calendar::Busy;

pub fn calendars() -> Vec<CalendarInfo> {
    Vec::new()
}

pub fn busy_between(_from_ms: i64, _to_ms: i64, _excluded: &[String]) -> Vec<Busy> {
    Vec::new()
}
