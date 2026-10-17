//! One-shot Markdown / CSV save of the Tasks list being viewed. Not a `Provider`: no account,
//! queue, target or remote id.

use std::collections::HashMap;
use std::str::FromStr;

use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone};
use rusqlite::Connection;

use crate::db::{categories, tasks};
use crate::error::{AppError, AppResult};
use crate::i18n::{tr, Lang};
use crate::model::TaskQuery;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FileFormat {
    Markdown,
    Csv,
}

impl FromStr for FileFormat {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "markdown" => Ok(Self::Markdown),
            "csv" => Ok(Self::Csv),
            _ => Err(AppError::Code("invalid")),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FileTask {
    pub title: String,
    pub done: bool,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub priority: i64,
    pub estimate_min: Option<i64>,
    pub category: String,
    pub first_step: Option<String>,
    pub notes: String,
    pub source: Option<String>,
    pub link: Option<String>,
    /// Local "YYYY-MM-DD HH:MM".
    pub created: String,
    /// Local "YYYY-MM-DD HH:MM".
    pub done_on: Option<String>,
}

fn local_time(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

/// The tasks `tasks::list` shows for `view` (open: all; done: newest 200), with notes, the category's
/// display name in `lang` (`categories::display_name`, cached per id), `source_title`, the exported
/// `external_url` (state `exported` only) and local timestamps.
pub fn rows(conn: &Connection, view: &str, lang: Lang) -> AppResult<Vec<FileTask>> {
    let list = tasks::list(conn, &TaskQuery { view: view.into(), search: None, agent_visible: false })?;
    let mut stmt = conn.prepare("SELECT id, notes FROM tasks WHERE deleted_at IS NULL AND status = ?1")?;
    let notes: HashMap<String, String> = stmt
        .query_map([view], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut names: HashMap<String, String> = HashMap::new();
    let mut out = Vec::with_capacity(list.len());
    for t in list {
        let category = match names.get(&t.category_id) {
            Some(n) => n.clone(),
            None => {
                let n = categories::display_name(conn, &t.category_id, lang)?;
                names.insert(t.category_id.clone(), n.clone());
                n
            }
        };
        let link = t.export.as_ref().filter(|e| e.state == "exported").and_then(|e| e.external_url.clone());
        out.push(FileTask {
            done: t.status == "done",
            notes: notes.get(&t.id).cloned().unwrap_or_default(),
            title: t.title,
            due_date: t.due_date,
            due_time: t.due_time,
            priority: t.priority,
            estimate_min: t.estimate_min,
            category,
            first_step: t.first_step,
            source: t.source_title,
            link,
            created: local_time(t.created_at),
            done_on: t.done_at.map(local_time),
        });
    }
    Ok(out)
}

/// Line breaks become spaces (single-line Markdown fields).
fn one_line(s: &str) -> String {
    s.replace("\r\n", " ").replace(['\r', '\n'], " ")
}

fn priority_key(p: i64) -> Option<&'static str> {
    match p {
        1 => Some("tasksFile.priority.low"),
        2 => Some("tasksFile.priority.medium"),
        3 => Some("tasksFile.priority.high"),
        _ => None,
    }
}

pub fn markdown(tasks: &[FileTask], done_view: bool, lang: Lang, saved: &str) -> String {
    let mut out = format!(
        "# {}\n{}\n",
        tr(lang, if done_view { "tasksFile.done" } else { "tasksFile.open" }),
        tr(lang, "tasksFile.saved").replace("{date}", saved)
    );
    for t in tasks {
        out.push_str(&format!("\n- [{}] {}\n", if t.done { "x" } else { " " }, one_line(&t.title)));
        let mut parts = Vec::new();
        if let Some(d) = &t.due_date {
            let mut s = format!("{}: {}", tr(lang, "tasksFile.due"), one_line(d));
            if let Some(time) = &t.due_time {
                s.push(' ');
                s.push_str(&one_line(time));
            }
            parts.push(s);
        }
        if let Some(d) = &t.done_on {
            parts.push(format!("{}: {}", tr(lang, "tasksFile.doneOn"), d.chars().take(10).collect::<String>()));
        }
        if let Some(k) = priority_key(t.priority) {
            parts.push(format!("{}: {}", tr(lang, "tasksFile.priority"), tr(lang, k)));
        }
        if let Some(n) = t.estimate_min {
            parts.push(format!(
                "{}: {}",
                tr(lang, "tasksFile.estimate"),
                tr(lang, "tasksFile.minutes").replace("{n}", &n.to_string())
            ));
        }
        parts.push(format!("{}: {}", tr(lang, "tasksFile.category"), one_line(&t.category)));
        out.push_str(&format!("  {}\n", parts.join(" · ")));
        for (key, value) in [
            ("tasksFile.firstStep", &t.first_step),
            ("tasksFile.source", &t.source),
            ("tasksFile.link", &t.link),
        ] {
            if let Some(v) = value {
                out.push_str(&format!("  {}: {}\n", tr(lang, key), one_line(v)));
            }
        }
        if !t.notes.trim().is_empty() {
            for line in t.notes.lines() {
                let line = line.trim_end();
                if line.is_empty() {
                    out.push_str("  >\n");
                } else {
                    out.push_str(&format!("  > {line}\n"));
                }
            }
        }
    }
    out
}

const CSV_HEADER: &str =
    "Title,Status,Due date,Due time,Priority,Estimate (min),Category,First step,Notes,Source,Link,Created,Done";

/// RFC 4180 with a UTF-8 BOM (Excel on macOS) and CRLF records; English header.
pub fn csv(tasks: &[FileTask]) -> String {
    let mut out = format!("\u{FEFF}{CSV_HEADER}\r\n");
    for t in tasks {
        let opt = |v: &Option<String>| v.as_deref().map(cell).unwrap_or_default();
        let fields = [
            cell(&t.title),
            (if t.done { "done" } else { "open" }).to_string(),
            opt(&t.due_date),
            opt(&t.due_time),
            match t.priority {
                1 => "low",
                2 => "medium",
                3 => "high",
                _ => "none",
            }
            .to_string(),
            t.estimate_min.map(|n| n.to_string()).unwrap_or_default(),
            cell(&t.category),
            opt(&t.first_step),
            cell(&t.notes),
            opt(&t.source),
            opt(&t.link),
            cell(&t.created),
            opt(&t.done_on),
        ];
        out.push_str(&fields.join(","));
        out.push_str("\r\n");
    }
    out
}

pub fn file_name(done_view: bool, format: FileFormat, lang: Lang, date: NaiveDate) -> String {
    format!(
        "{} {}.{}",
        tr(lang, if done_view { "tasksFile.nameDone" } else { "tasksFile.nameOpen" }),
        date.format("%Y-%m-%d"),
        match format {
            FileFormat::Markdown => "md",
            FileFormat::Csv => "csv",
        }
    )
}

/// (file name, text). `view` other than open|done → `invalid` (from `tasks::list`).
pub fn build(
    conn: &Connection,
    view: &str,
    format: FileFormat,
    lang: Lang,
    now: NaiveDateTime,
) -> AppResult<(String, String)> {
    let tasks = rows(conn, view, lang)?;
    let done_view = view == "done";
    let text = match format {
        FileFormat::Markdown => markdown(&tasks, done_view, lang, &now.format("%Y-%m-%d %H:%M").to_string()),
        FileFormat::Csv => csv(&tasks),
    };
    Ok((file_name(done_view, format, lang, now.date()), text))
}

/// One CSV value: formula-leading values get a `'`; values with `,` `"` CR LF or edge spaces are quoted.
fn cell(s: &str) -> String {
    let s = if s.starts_with(['=', '+', '-', '@']) { format!("'{s}") } else { s.to_string() };
    if s.contains([',', '"', '\r', '\n']) || s.starts_with(' ') || s.ends_with(' ') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn write_spec() -> FileTask {
        FileTask {
            title: "Write spec".into(),
            due_date: Some("2026-10-20".into()),
            due_time: Some("09:30".into()),
            priority: 3,
            estimate_min: Some(30),
            category: "To-do".into(),
            first_step: Some("Outline the API".into()),
            notes: "Line one\nLine two".into(),
            source: Some("API contract".into()),
            link: Some("https://app.asana.com/0/1/9".into()),
            created: "2026-10-17 09:00".into(),
            ..Default::default()
        }
    }

    #[test]
    fn csv_cells_follow_rfc4180() {
        assert_eq!(cell("a,b"), "\"a,b\"");
        assert_eq!(cell("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(cell("x\ny"), "\"x\ny\"");
        assert_eq!(cell("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(cell(" pad"), "\" pad\"");
        assert_eq!(cell("plain"), "plain");
        assert_eq!(cell(""), "");
    }

    #[test]
    fn csv_file_shape() {
        let text = csv(&[write_spec()]);
        let head = format!("\u{FEFF}{CSV_HEADER}\r\n");
        assert!(text.starts_with(&head));
        assert_eq!(
            &text[head.len()..],
            "Write spec,open,2026-10-20,09:30,high,30,To-do,Outline the API,\"Line one\nLine two\",API contract,https://app.asana.com/0/1/9,2026-10-17 09:00,\r\n"
        );
    }

    #[test]
    fn markdown_open_view() {
        let call = FileTask { title: "Call Lan".into(), category: "Contact".into(), ..Default::default() };
        let both = [write_spec(), call];
        assert_eq!(
            markdown(&both, false, Lang::En, "2026-10-17 14:02"),
            "# Open tasks\nSaved from Co-pet · 2026-10-17 14:02\n\n- [ ] Write spec\n  Due: 2026-10-20 09:30 · Priority: High · Estimate: 30 min · Category: To-do\n  First step: Outline the API\n  Source: API contract\n  Link: https://app.asana.com/0/1/9\n  > Line one\n  > Line two\n\n- [ ] Call Lan\n  Category: Contact\n"
        );
    }

    #[test]
    fn markdown_done_view_vi() {
        let t = FileTask {
            title: "Gửi báo giá".into(),
            done: true,
            done_on: Some("2026-10-16 17:45".into()),
            category: "Việc cần làm".into(),
            ..Default::default()
        };
        assert_eq!(
            markdown(&[t], true, Lang::Vi, "2026-10-17 14:02"),
            "# Công việc đã xong\nLưu từ Co-pet · 2026-10-17 14:02\n\n- [x] Gửi báo giá\n  Xong: 2026-10-16 · Danh mục: Việc cần làm\n"
        );
    }

    #[test]
    fn file_names() {
        let d = NaiveDate::from_ymd_opt(2026, 10, 17).unwrap();
        assert_eq!(file_name(false, FileFormat::Markdown, Lang::En, d), "Co-pet open tasks 2026-10-17.md");
        assert_eq!(file_name(true, FileFormat::Csv, Lang::Vi, d), "Co-pet việc đã xong 2026-10-17.csv");
    }

    #[test]
    fn build_rejects_unknown_view_and_format() {
        let db = Db::open_in_memory().unwrap();
        let now = NaiveDate::from_ymd_opt(2026, 10, 17).unwrap().and_hms_opt(14, 2, 0).unwrap();
        let err = build(&db.conn(), "later", FileFormat::Csv, Lang::En, now).unwrap_err();
        assert!(matches!(err, AppError::Code("invalid")));
        assert!(matches!("pdf".parse::<FileFormat>(), Err(AppError::Code("invalid"))));
    }
}
