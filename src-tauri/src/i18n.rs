//! Rust-side strings (screenshot titles, menu bar labels, built-in category names). The UI's
//! strings live in `src/shared/i18n/`.
use chrono::{Datelike, NaiveDate};
use tauri::AppHandle;

use crate::db::settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Vi,
    Zh,
    Es,
}

impl Lang {
    pub const ALL: [Lang; 4] = [Lang::En, Lang::Vi, Lang::Zh, Lang::Es];

    /// The `ui.language` / summary language code.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Vi => "vi",
            Lang::Zh => "zh",
            Lang::Es => "es",
        }
    }

    /// The language whose code `tag` starts with (`zh-Hans-CN` → `Zh`).
    fn from_tag(tag: &str) -> Option<Lang> {
        let tag = tag.to_ascii_lowercase();
        Lang::ALL.into_iter().find(|l| tag.starts_with(l.code()))
    }
}

/// Resolves `ui.language`; `system` follows the first OS preferred language.
pub fn lang(app: &AppHandle) -> Lang {
    match settings::read::<String>(app, "ui.language") {
        Ok(code) => Lang::ALL.into_iter().find(|l| l.code() == code).unwrap_or_else(system_lang),
        Err(_) => system_lang(),
    }
}

/// The OS's first preferred language when Co-pet has it, else `En`.
pub fn system_lang() -> Lang {
    crate::platform::locale::preferred_language().as_deref().and_then(Lang::from_tag).unwrap_or(Lang::En)
}

/// The string for `key`. Keys are fixed in code, so an unknown key is a programming error.
pub fn tr(lang: Lang, key: &str) -> &'static str {
    let (en, vi, zh, es) = match key {
        "screenshot.title" => ("Screenshot", "Ảnh chụp", "截图", "Captura de pantalla"),
        "image.title" => ("Image", "Hình ảnh", "图片", "Imagen"),
        "tray.openHome" => ("Open home", "Mở trang chính", "打开主页", "Abrir inicio"),
        "tray.showPet" => ("Show pet", "Hiện thú cưng", "显示宠物", "Mostrar mascota"),
        "tray.pausePet" => ("Pause pet", "Tạm dừng thú cưng", "暂停宠物", "Pausar mascota"),
        "tray.resumePet" => ("Resume pet", "Tiếp tục thú cưng", "继续宠物", "Reanudar mascota"),
        "tray.settings" => ("Settings", "Cài đặt", "设置", "Ajustes"),
        "tray.quit" => ("Quit Co-pet", "Thoát Co-pet", "退出 Co-pet", "Salir de Co-pet"),
        "quit.recording.title" => (
            "Stop recording and quit?",
            "Dừng ghi âm và thoát?",
            "停止录制并退出？",
            "¿Detener la grabación y salir?",
        ),
        "quit.recording.body" => (
            "The recording so far is kept and transcribed the next time Co-pet opens.",
            "Phần đã ghi được giữ lại và chuyển thành văn bản khi Co-pet mở lại.",
            "已录制的内容会保留，并在下次打开 Co-pet 时转录。",
            "Lo grabado hasta ahora se guarda y se transcribe la próxima vez que abras Co-pet.",
        ),
        "quit.recording.ok" => ("Stop and quit", "Dừng và thoát", "停止并退出", "Detener y salir"),
        "quit.recording.cancel" => ("Keep recording", "Tiếp tục ghi", "继续录制", "Seguir grabando"),
        "category.todo" => ("To-do", "Việc cần làm", "待办", "Por hacer"),
        "category.read" => ("Read later", "Đọc sau", "稍后阅读", "Leer después"),
        "category.reference" => ("Reference", "Tài liệu", "参考资料", "Referencia"),
        "category.code" => ("Code snippet", "Đoạn code", "代码片段", "Fragmento de código"),
        "category.design" => ("Design", "Thiết kế", "设计", "Diseño"),
        "category.idea" => ("Idea / note", "Ý tưởng / ghi chú", "想法 / 笔记", "Idea / nota"),
        "category.contact" => ("Contact", "Liên hệ", "联系人", "Contacto"),
        "category.meeting" => ("Meeting", "Cuộc họp", "会议", "Reunión"),
        "category.unsorted" => ("Unsorted", "Chưa phân loại", "未分类", "Sin clasificar"),
        "meeting.defaultTitle" => ("Meeting", "Cuộc họp", "会议", "Reunión"),
        "meeting.titleZoom" => ("Zoom call", "Cuộc gọi Zoom", "Zoom 通话", "Llamada de Zoom"),
        "meeting.titleMeet" => ("Google Meet: {name}", "Google Meet: {name}", "Google Meet：{name}", "Google Meet: {name}"),
        "meeting.titleMeetPlain" => (
            "Google Meet call",
            "Cuộc gọi Google Meet",
            "Google Meet 通话",
            "Llamada de Google Meet",
        ),
        "meeting.me" => ("Me", "Tôi", "我", "Yo"),
        "meeting.others" => ("Others", "Người khác", "其他人", "Otros"),
        "meeting.speaker" => ("Speaker {n}", "Người nói {n}", "发言人 {n}", "Hablante {n}"),
        "meeting.summary" => ("Summary", "Tóm tắt", "摘要", "Resumen"),
        "meeting.decisions" => ("Decisions", "Quyết định", "决定", "Decisiones"),
        "meeting.actions" => ("Action items", "Việc cần làm", "待办事项", "Acciones"),
        "meeting.questions" => ("Open questions", "Câu hỏi còn mở", "待解决的问题", "Preguntas abiertas"),
        "meeting.transcript" => ("Transcript", "Bản ghi", "转录文本", "Transcripción"),
        "meeting.due" => ("due", "hạn", "截止", "vence"),
        "meeting.taskFrom" => (
            "From “{title}” · {date}",
            "Từ “{title}” · {date}",
            "来自“{title}” · {date}",
            "De “{title}” · {date}",
        ),
        "meeting.owner" => ("Owner: {name}", "Người phụ trách: {name}", "负责人：{name}", "Responsable: {name}"),
        "lang.en" => ("English", "Tiếng Anh", "英语", "Inglés"),
        "lang.vi" => ("Vietnamese", "Tiếng Việt", "越南语", "Vietnamita"),
        "lang.zh" => ("Chinese", "Tiếng Trung", "中文", "Chino"),
        "lang.es" => ("Spanish", "Tiếng Tây Ban Nha", "西班牙语", "Español"),
        "tasksFile.open" => ("Open tasks", "Công việc đang mở", "未完成的任务", "Tareas abiertas"),
        "tasksFile.done" => ("Done tasks", "Công việc đã xong", "已完成的任务", "Tareas hechas"),
        "tasksFile.saved" => (
            "Saved from Co-pet · {date}",
            "Lưu từ Co-pet · {date}",
            "保存自 Co-pet · {date}",
            "Guardado desde Co-pet · {date}",
        ),
        "tasksFile.due" => ("Due", "Hạn", "截止", "Vence"),
        "tasksFile.doneOn" => ("Done", "Xong", "完成", "Hecha"),
        "tasksFile.priority" => ("Priority", "Độ ưu tiên", "优先级", "Prioridad"),
        "tasksFile.priority.low" => ("Low", "Thấp", "低", "Baja"),
        "tasksFile.priority.medium" => ("Medium", "Trung bình", "中", "Media"),
        "tasksFile.priority.high" => ("High", "Cao", "高", "Alta"),
        "tasksFile.estimate" => ("Estimate", "Ước tính", "预计", "Estimación"),
        "tasksFile.minutes" => ("{n} min", "{n} phút", "{n} 分钟", "{n} min"),
        "tasksFile.category" => ("Category", "Danh mục", "分类", "Categoría"),
        "tasksFile.firstStep" => ("First step", "Bước đầu tiên", "第一步", "Primer paso"),
        "tasksFile.source" => ("Source", "Nguồn", "来源", "Origen"),
        "tasksFile.link" => ("Link", "Liên kết", "链接", "Enlace"),
        "tasksFile.nameOpen" => ("Co-pet open tasks", "Co-pet việc đang mở", "Co-pet 未完成任务", "Co-pet tareas abiertas"),
        "tasksFile.nameDone" => ("Co-pet done tasks", "Co-pet việc đã xong", "Co-pet 已完成任务", "Co-pet tareas hechas"),
        "storage.missingTitle" => (
            "Data folder not found",
            "Không tìm thấy thư mục dữ liệu",
            "找不到数据文件夹",
            "No se encuentra la carpeta de datos",
        ),
        "storage.missing" => (
            "Co-pet can't find its data folder: {path}. Connect that drive, then open Co-pet again.",
            "Co-pet không tìm thấy thư mục dữ liệu: {path}. Hãy kết nối ổ đĩa đó rồi mở lại Co-pet.",
            "Co-pet 找不到它的数据文件夹：{path}。请连接该磁盘，然后重新打开 Co-pet。",
            "Co-pet no encuentra su carpeta de datos: {path}. Conecta esa unidad y vuelve a abrir Co-pet.",
        ),
        _ => {
            debug_assert!(false, "unknown i18n key {key}");
            ("", "", "", "")
        }
    };
    match lang {
        Lang::En => en,
        Lang::Vi => vi,
        Lang::Zh => zh,
        Lang::Es => es,
    }
}

/// Matches `src/shared/format.ts formatDate`: en `Thu 15 Oct`, vi `T5, 15/10` (Sunday `CN`),
/// zh `10月15日 周四`, es `jue 15 oct`.
pub fn short_date(lang: Lang, d: NaiveDate) -> String {
    const EN_DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const VI_DAYS: [&str; 7] = ["CN", "T2", "T3", "T4", "T5", "T6", "T7"];
    const ZH_DAYS: [&str; 7] = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
    const ES_DAYS: [&str; 7] = ["dom", "lun", "mar", "mié", "jue", "vie", "sáb"];
    const EN_MONTHS: [&str; 12] =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    const ES_MONTHS: [&str; 12] =
        ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic"];
    let wd = d.weekday().num_days_from_sunday() as usize;
    let m0 = d.month0() as usize;
    match lang {
        Lang::Vi => format!("{}, {}/{}", VI_DAYS[wd], d.day(), d.month()),
        Lang::Zh => format!("{}月{}日 {}", d.month(), d.day(), ZH_DAYS[wd]),
        Lang::Es => format!("{} {} {}", ES_DAYS[wd], d.day(), ES_MONTHS[m0]),
        Lang::En => format!("{} {} {}", EN_DAYS[wd], d.day(), EN_MONTHS[m0]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_tags_map_to_ui_languages() {
        assert_eq!(Lang::from_tag("zh-Hans-CN"), Some(Lang::Zh));
        assert_eq!(Lang::from_tag("zh-TW"), Some(Lang::Zh));
        assert_eq!(Lang::from_tag("es-419"), Some(Lang::Es));
        assert_eq!(Lang::from_tag("vi-VN"), Some(Lang::Vi));
        assert_eq!(Lang::from_tag("fr-FR"), None);
    }

    #[test]
    fn short_date_matches_format_ts() {
        let d = NaiveDate::from_ymd_opt(2025, 10, 17).unwrap();
        assert_eq!(short_date(Lang::En, d), "Fri 17 Oct");
        assert_eq!(short_date(Lang::Vi, d), "T6, 17/10");
        assert_eq!(short_date(Lang::Zh, d), "10月17日 周五");
        assert_eq!(short_date(Lang::Es, d), "vie 17 oct");
    }
}
