//! Rust-side strings (screenshot titles, menu bar labels, built-in category names). The UI's
//! strings live in `src/shared/i18n/`.
use tauri::AppHandle;

use crate::db::settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Vi,
}

/// Resolves `ui.language`; `system` follows the first macOS preferred language.
pub fn lang(app: &AppHandle) -> Lang {
    match settings::read::<String>(app, "ui.language").as_deref() {
        Ok("en") => Lang::En,
        Ok("vi") => Lang::Vi,
        _ => system_lang(),
    }
}

/// `Vi` when `NSLocale.preferredLanguages[0]` starts with `vi`, else `En`.
pub fn system_lang() -> Lang {
    let first = objc2_foundation::NSLocale::preferredLanguages().firstObject();
    match first {
        Some(code) if code.to_string().starts_with("vi") => Lang::Vi,
        _ => Lang::En,
    }
}

/// The string for `key`. Keys are fixed in code, so an unknown key is a programming error.
pub fn tr(lang: Lang, key: &str) -> &'static str {
    let (en, vi) = match key {
        "screenshot.title" => ("Screenshot", "Ảnh chụp"),
        "image.title" => ("Image", "Hình ảnh"),
        "tray.openHome" => ("Open home", "Mở trang chính"),
        "tray.showPet" => ("Show pet", "Hiện thú cưng"),
        "tray.pausePet" => ("Pause pet", "Tạm dừng thú cưng"),
        "tray.resumePet" => ("Resume pet", "Tiếp tục thú cưng"),
        "tray.settings" => ("Settings", "Cài đặt"),
        "tray.quit" => ("Quit Co-pet", "Thoát Co-pet"),
        "category.todo" => ("To-do", "Việc cần làm"),
        "category.read" => ("Read later", "Đọc sau"),
        "category.reference" => ("Reference", "Tài liệu"),
        "category.code" => ("Code snippet", "Đoạn code"),
        "category.design" => ("Design", "Thiết kế"),
        "category.idea" => ("Idea / note", "Ý tưởng / ghi chú"),
        "category.contact" => ("Contact", "Liên hệ"),
        "category.meeting" => ("Meeting", "Cuộc họp"),
        "category.unsorted" => ("Unsorted", "Chưa phân loại"),
        _ => {
            debug_assert!(false, "unknown i18n key {key}");
            ("", "")
        }
    };
    match lang {
        Lang::En => en,
        Lang::Vi => vi,
    }
}
