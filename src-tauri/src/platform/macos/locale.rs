//! The user's preferred UI language.

/// `NSLocale.preferredLanguages[0]` (e.g. `vi-VN`); `None` when the list is empty.
pub fn preferred_language() -> Option<String> {
    objc2_foundation::NSLocale::preferredLanguages().firstObject().map(|s| s.to_string())
}
