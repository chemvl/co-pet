//! Input idle time for `platform::idle`.

/// `kCGEventSourceStateHIDSystemState`.
const HID_SYSTEM_STATE: i32 = 1;
/// `kCGAnyInputEventType`.
const ANY_INPUT_EVENT: u32 = !0;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceSecondsSinceLastEventType(state_id: i32, event_type: u32) -> f64;
}

/// Seconds since the last keyboard/mouse input from any source.
pub fn seconds_since_input() -> f64 {
    // SAFETY: plain C call with constant arguments.
    unsafe { CGEventSourceSecondsSinceLastEventType(HID_SYSTEM_STATE, ANY_INPUT_EVENT) }
}
