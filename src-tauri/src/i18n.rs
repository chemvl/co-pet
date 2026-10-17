//! Rust-side strings (screenshot titles, menu bar labels, built-in category names). The UI's
//! strings live in `src/shared/i18n/`.
use chrono::{Datelike, NaiveDate};
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
        "tray.restartToUpdate" => ("Restart to update to {version}", "Khởi động lại để cập nhật lên {version}"),
        "tray.quit" => ("Quit Co-pet", "Thoát Co-pet"),
        "quit.recording.title" => ("Stop recording and quit?", "Dừng ghi âm và thoát?"),
        "quit.recording.body" => (
            "The recording so far is kept and transcribed the next time Co-pet opens.",
            "Phần đã ghi được giữ lại và chuyển thành văn bản khi Co-pet mở lại.",
        ),
        "quit.recording.ok" => ("Stop and quit", "Dừng và thoát"),
        "quit.recording.cancel" => ("Keep recording", "Tiếp tục ghi"),
        "update.recording" => (
            "Stop the recording before restarting.",
            "Hãy dừng ghi âm trước khi khởi động lại.",
        ),
        "category.todo" => ("To-do", "Việc cần làm"),
        "category.read" => ("Read later", "Đọc sau"),
        "category.reference" => ("Reference", "Tài liệu"),
        "category.code" => ("Code snippet", "Đoạn code"),
        "category.design" => ("Design", "Thiết kế"),
        "category.idea" => ("Idea / note", "Ý tưởng / ghi chú"),
        "category.contact" => ("Contact", "Liên hệ"),
        "category.meeting" => ("Meeting", "Cuộc họp"),
        "category.unsorted" => ("Unsorted", "Chưa phân loại"),
        "meeting.defaultTitle" => ("Meeting", "Cuộc họp"),
        "meeting.me" => ("Me", "Tôi"),
        "meeting.others" => ("Others", "Người khác"),
        "meeting.speaker" => ("Speaker {n}", "Người nói {n}"),
        "meeting.summary" => ("Summary", "Tóm tắt"),
        "meeting.decisions" => ("Decisions", "Quyết định"),
        "meeting.actions" => ("Action items", "Việc cần làm"),
        "meeting.questions" => ("Open questions", "Câu hỏi còn mở"),
        "meeting.transcript" => ("Transcript", "Bản ghi"),
        "meeting.due" => ("due", "hạn"),
        "meeting.taskFrom" => ("From “{title}” · {date}", "Từ “{title}” · {date}"),
        "meeting.owner" => ("Owner: {name}", "Người phụ trách: {name}"),
        "lang.en" => ("English", "Tiếng Anh"),
        "lang.vi" => ("Vietnamese", "Tiếng Việt"),
        "lang.zh" => ("Chinese", "Tiếng Trung"),
        "tasksFile.open" => ("Open tasks", "Công việc đang mở"),
        "tasksFile.done" => ("Done tasks", "Công việc đã xong"),
        "tasksFile.saved" => ("Saved from Co-pet · {date}", "Lưu từ Co-pet · {date}"),
        "tasksFile.due" => ("Due", "Hạn"),
        "tasksFile.doneOn" => ("Done", "Xong"),
        "tasksFile.priority" => ("Priority", "Độ ưu tiên"),
        "tasksFile.priority.low" => ("Low", "Thấp"),
        "tasksFile.priority.medium" => ("Medium", "Trung bình"),
        "tasksFile.priority.high" => ("High", "Cao"),
        "tasksFile.estimate" => ("Estimate", "Ước tính"),
        "tasksFile.minutes" => ("{n} min", "{n} phút"),
        "tasksFile.category" => ("Category", "Danh mục"),
        "tasksFile.firstStep" => ("First step", "Bước đầu tiên"),
        "tasksFile.source" => ("Source", "Nguồn"),
        "tasksFile.link" => ("Link", "Liên kết"),
        "tasksFile.nameOpen" => ("Co-pet open tasks", "Co-pet việc đang mở"),
        "tasksFile.nameDone" => ("Co-pet done tasks", "Co-pet việc đã xong"),
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

/// Matches `src/shared/format.ts formatDate`: en `Thu 15 Oct`, vi `T5, 15/10` (Sunday `CN`).
pub fn short_date(lang: Lang, d: NaiveDate) -> String {
    const EN_DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const VI_DAYS: [&str; 7] = ["CN", "T2", "T3", "T4", "T5", "T6", "T7"];
    const EN_MONTHS: [&str; 12] =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let wd = d.weekday().num_days_from_sunday() as usize;
    match lang {
        Lang::Vi => format!("{}, {}/{}", VI_DAYS[wd], d.day(), d.month()),
        Lang::En => format!("{} {} {}", EN_DAYS[wd], d.day(), EN_MONTHS[d.month0() as usize]),
    }
}
