//! Idle time for `platform::idle`.
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

/// Seconds since the last keyboard or mouse input in this session.
pub fn seconds_since_input() -> f64 {
    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return 0.0;
    }
    // `dwTime` is a 32-bit tick count: compare in wrapping u32 arithmetic.
    let now = unsafe { GetTickCount64() } as u32;
    f64::from(now.wrapping_sub(info.dwTime)) / 1000.0
}
