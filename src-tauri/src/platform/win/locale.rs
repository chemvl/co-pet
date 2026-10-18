//! The user's preferred UI language.
use windows::core::PWSTR;
use windows::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};

/// First entry of the user's preferred UI languages, e.g. `vi-VN`.
pub fn preferred_language() -> Option<String> {
    let (mut count, mut len) = (0u32, 0u32);
    unsafe {
        GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut len).ok()?;
        let mut buf = vec![0u16; len as usize];
        GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, Some(PWSTR(buf.as_mut_ptr())), &mut len).ok()?;
        let first = super::from_wide(&buf);
        (!first.is_empty()).then_some(first)
    }
}
