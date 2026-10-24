//! Meeting reads and writes. JSON columns hold the camelCase shapes of `model`'s meeting types;
//! every write bumps `updated_at`.
use chrono::{Local, TimeZone};
use rusqlite::types::Type;
use rusqlite::{params, Connection, OptionalExtension, Row, ToSql};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tauri::AppHandle;

use crate::db::{new_id, now_ms};
use crate::error::{AppError, AppResult};
use crate::events;
use crate::i18n::{short_date, tr, Lang};
use crate::model::{ActionItem, MeetingDetail, MeetingSummary, Speaker, TaskChange, TranscriptSeg};
use crate::paths::Paths;

/// Every accepted meeting language (single source for language checks).
pub const LANGUAGES: [&str; 5] = ["auto", "en", "vi", "zh", "vi+en"];

const LIST_LIMIT: i64 = 500;

const COLS: &str = "id, title, title_source, source, source_item_id, device_id, started_at, duration_ms, \
    language, langs, state, error, has_mic, has_system, diarized, speakers, transcript, summary_state, \
    summary_error, summary_lang, summary, decisions, questions, actions, changes, tasks_created_at, \
    audio_deleted_at, created_at, updated_at, deleted_at";

/// One `meetings` row, JSON columns decoded.
#[derive(Debug, Clone, PartialEq)]
pub struct MeetingRow {
    pub id: String,
    pub title: String,
    pub title_source: String,
    pub source: String,
    pub source_item_id: Option<String>,
    pub device_id: String,
    pub started_at: i64,
    pub duration_ms: i64,
    pub language: String,
    pub langs: Vec<String>,
    pub state: String,
    pub error: Option<String>,
    pub has_mic: bool,
    pub has_system: bool,
    pub diarized: bool,
    pub speakers: Vec<Speaker>,
    pub transcript: Vec<TranscriptSeg>,
    pub summary_state: String,
    pub summary_error: Option<String>,
    pub summary_lang: Option<String>,
    pub summary: Vec<String>,
    pub decisions: Vec<String>,
    pub questions: Vec<String>,
    pub actions: Vec<ActionItem>,
    pub changes: Vec<TaskChange>,
    pub tasks_created_at: Option<i64>,
    pub audio_deleted_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
}

fn json_col<T: DeserializeOwned>(r: &Row<'_>, i: usize) -> rusqlite::Result<T> {
    let s: String = r.get(i)?;
    serde_json::from_str(&s).map_err(|e| rusqlite::Error::FromSqlConversionFailure(i, Type::Text, Box::new(e)))
}

fn map_row(r: &Row<'_>) -> rusqlite::Result<MeetingRow> {
    Ok(MeetingRow {
        id: r.get(0)?,
        title: r.get(1)?,
        title_source: r.get(2)?,
        source: r.get(3)?,
        source_item_id: r.get(4)?,
        device_id: r.get(5)?,
        started_at: r.get(6)?,
        duration_ms: r.get(7)?,
        language: r.get(8)?,
        langs: json_col(r, 9)?,
        state: r.get(10)?,
        error: r.get(11)?,
        has_mic: r.get(12)?,
        has_system: r.get(13)?,
        diarized: r.get(14)?,
        speakers: json_col(r, 15)?,
        transcript: json_col(r, 16)?,
        summary_state: r.get(17)?,
        summary_error: r.get(18)?,
        summary_lang: r.get(19)?,
        summary: json_col(r, 20)?,
        decisions: json_col(r, 21)?,
        questions: json_col(r, 22)?,
        actions: json_col(r, 23)?,
        changes: json_col(r, 24)?,
        tasks_created_at: r.get(25)?,
        audio_deleted_at: r.get(26)?,
        created_at: r.get(27)?,
        updated_at: r.get(28)?,
        deleted_at: r.get(29)?,
    })
}

fn to_json<T: Serialize + ?Sized>(v: &T) -> AppResult<String> {
    Ok(serde_json::to_string(v)?)
}

/// Runs `UPDATE meetings SET {sets}, updated_at = ?1 WHERE id = ?2` on a live meeting; `values`
/// bind from `?3`. `notFound` when no live row matched.
fn update(conn: &Connection, id: &str, sets: &str, values: &[&dyn ToSql]) -> AppResult<()> {
    let now = now_ms();
    let mut all: Vec<&dyn ToSql> = vec![&now, &id];
    all.extend_from_slice(values);
    let n = conn.execute(
        &format!("UPDATE meetings SET {sets}, updated_at = ?1 WHERE id = ?2 AND deleted_at IS NULL"),
        all.as_slice(),
    )?;
    if n == 0 {
        return Err(AppError::Code("notFound"));
    }
    Ok(())
}

// ---------- writes ----------

/// New meeting: state `recording` for a recording, `queued` for an import.
pub fn create(
    conn: &Connection,
    source: &str,
    language: &str,
    device_id: &str,
    started_at: i64,
    title: &str,
    source_item_id: Option<&str>,
) -> AppResult<String> {
    let state = match source {
        "recording" => "recording",
        "import" => "queued",
        _ => return Err(AppError::Code("invalid")),
    };
    if !LANGUAGES.contains(&language) {
        return Err(AppError::Code("invalid"));
    }
    let id = new_id();
    let now = now_ms();
    conn.execute(
        "INSERT INTO meetings (id, title, source, source_item_id, device_id, started_at, language, state,
           created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        params![id, title, source, source_item_id, device_id, started_at, language, state, now],
    )?;
    Ok(id)
}

pub fn set_state(conn: &Connection, id: &str, state: &str, error: Option<&str>) -> AppResult<()> {
    update(conn, id, "state = ?3, error = ?4", &[&state, &error])
}

pub fn set_duration(conn: &Connection, id: &str, duration_ms: i64, has_mic: bool, has_system: bool) -> AppResult<()> {
    update(conn, id, "duration_ms = ?3, has_mic = ?4, has_system = ?5", &[&duration_ms, &has_mic, &has_system])
}

pub fn set_language(conn: &Connection, id: &str, language: &str) -> AppResult<()> {
    if !LANGUAGES.contains(&language) {
        return Err(AppError::Code("invalid"));
    }
    update(conn, id, "language = ?3", &[&language])
}

pub fn save_transcript(
    conn: &Connection,
    id: &str,
    segs: &[TranscriptSeg],
    speakers: &[Speaker],
    langs: &[String],
    diarized: bool,
) -> AppResult<()> {
    let (segs, speakers, langs) = (to_json(segs)?, to_json(speakers)?, to_json(langs)?);
    update(
        conn,
        id,
        "transcript = ?3, speakers = ?4, langs = ?5, diarized = ?6",
        &[&segs, &speakers, &langs, &diarized],
    )
}

/// Stores the AI notes. `title` (the AI title) replaces only an automatic title and marks it `ai`.
pub fn save_notes(
    conn: &Connection,
    id: &str,
    title: Option<&str>,
    summary: &[String],
    decisions: &[String],
    questions: &[String],
    lang: &str,
) -> AppResult<()> {
    let (summary, decisions, questions) = (to_json(summary)?, to_json(decisions)?, to_json(questions)?);
    update(
        conn,
        id,
        "title = CASE WHEN title_source = 'auto' AND ?3 IS NOT NULL THEN ?3 ELSE title END,
         title_source = CASE WHEN title_source = 'auto' AND ?3 IS NOT NULL THEN 'ai' ELSE title_source END,
         summary = ?4, decisions = ?5, questions = ?6, summary_lang = ?7",
        &[&title, &summary, &decisions, &questions, &lang],
    )
}

pub fn save_actions(conn: &Connection, id: &str, actions: &[ActionItem]) -> AppResult<()> {
    let actions = to_json(actions)?;
    update(conn, id, "actions = ?3", &[&actions])
}

pub fn save_changes(conn: &Connection, id: &str, changes: &[TaskChange]) -> AppResult<()> {
    let changes = to_json(changes)?;
    update(conn, id, "changes = ?3", &[&changes])
}

pub fn set_summary_state(conn: &Connection, id: &str, state: &str, error: Option<&str>) -> AppResult<()> {
    update(conn, id, "summary_state = ?3, summary_error = ?4", &[&state, &error])
}

/// Records that the first summary's action items became tasks (set once).
pub fn mark_tasks_created(conn: &Connection, id: &str) -> AppResult<()> {
    update(conn, id, "tasks_created_at = COALESCE(tasks_created_at, ?1)", &[])
}

/// A better automatic title (the detected call's): replaces only an automatic title and keeps it
/// `auto`, so the AI title can still replace it.
pub fn set_auto_title(conn: &Connection, id: &str, title: &str) -> AppResult<()> {
    update(conn, id, "title = CASE WHEN title_source = 'auto' THEN ?3 ELSE title END", &[&title])
}

/// User rename: trimmed, 1–200 characters.
pub fn rename(conn: &Connection, id: &str, title: &str) -> AppResult<()> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > 200 {
        return Err(AppError::Code("invalid"));
    }
    update(conn, id, "title = ?3, title_source = 'user'", &[&title])
}

/// Names a speaker; empty or `None` restores the default label. `me` and unknown keys are
/// `invalid`; names are trimmed, at most 60 characters.
pub fn rename_speaker(conn: &Connection, id: &str, key: &str, name: Option<&str>) -> AppResult<()> {
    let mut speakers = get(conn, id)?.speakers;
    let sp = match speakers.iter_mut().find(|s| s.key == key) {
        Some(sp) if key != "me" => sp,
        _ => return Err(AppError::Code("invalid")),
    };
    let name = name.map(str::trim).filter(|n| !n.is_empty());
    if name.is_some_and(|n| n.chars().count() > 60) {
        return Err(AppError::Code("invalid"));
    }
    sp.name = name.map(String::from);
    let speakers = to_json(&speakers)?;
    update(conn, id, "speakers = ?3", &[&speakers])
}

pub fn soft_delete(conn: &Connection, id: &str) -> AppResult<()> {
    update(conn, id, "deleted_at = ?1", &[])
}

pub fn set_audio_deleted(conn: &Connection, id: &str, at: i64) -> AppResult<()> {
    update(conn, id, "audio_deleted_at = ?3", &[&at])
}

/// Emits `meetings-changed` for `ids`, then wakes sync.
pub fn notify_changed(app: &AppHandle, ids: Vec<String>) {
    events::meetings_changed(app, ids);
    crate::sync::poke(app);
}

// ---------- reads ----------

/// A live meeting; `notFound` when missing or deleted.
pub fn get(conn: &Connection, id: &str) -> AppResult<MeetingRow> {
    conn.query_row(&format!("SELECT {COLS} FROM meetings WHERE id = ?1 AND deleted_at IS NULL"), [id], map_row)
        .optional()?
        .ok_or(AppError::Code("notFound"))
}

fn ids(conn: &Connection, sql: &str, device_id: &str) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([device_id], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// This device's queued meetings, oldest first.
pub fn queued_for(conn: &Connection, device_id: &str) -> AppResult<Vec<String>> {
    ids(
        conn,
        "SELECT id FROM meetings WHERE device_id = ?1 AND state = 'queued' AND deleted_at IS NULL
         ORDER BY created_at, id",
        device_id,
    )
}

/// This device's transcribed meetings waiting for a summary, oldest first.
pub fn pending_summaries(conn: &Connection, device_id: &str) -> AppResult<Vec<String>> {
    ids(
        conn,
        "SELECT id FROM meetings WHERE device_id = ?1 AND state = 'done' AND summary_state = 'pending'
           AND deleted_at IS NULL
         ORDER BY created_at, id",
        device_id,
    )
}

/// This device's live meetings in `state`, oldest first.
pub fn in_state_for(conn: &Connection, device_id: &str, state: &str) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM meetings WHERE device_id = ?1 AND state = ?2 AND deleted_at IS NULL ORDER BY created_at, id",
    )?;
    let rows = stmt.query_map([device_id, state], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// This device's meetings by summary state and/or summary error (`None` matches any).
pub fn summary_in_state_for(
    conn: &Connection,
    device_id: &str,
    state: Option<&str>,
    error: Option<&str>,
) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM meetings WHERE device_id = ?1 AND (?2 IS NULL OR summary_state = ?2)
           AND (?3 IS NULL OR summary_error = ?3) AND deleted_at IS NULL
         ORDER BY created_at, id",
    )?;
    let rows = stmt.query_map(params![device_id, state, error], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// This device's finished (`done` | `failed`) meetings that still have audio and started
/// before `before`.
pub fn audio_expired(conn: &Connection, device_id: &str, before: i64) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM meetings WHERE device_id = ?1 AND state IN ('done', 'failed')
           AND audio_deleted_at IS NULL AND started_at < ?2 AND deleted_at IS NULL
         ORDER BY started_at, id",
    )?;
    let rows = stmt.query_map(params![device_id, before], |r| r.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The live item summarizing the meeting.
pub fn item_id(conn: &Connection, meeting_id: &str) -> AppResult<Option<String>> {
    Ok(conn
        .prepare_cached(
            "SELECT id FROM items WHERE meeting_id = ?1 AND deleted_at IS NULL ORDER BY captured_at LIMIT 1",
        )?
        .query_row([meeting_id], |r| r.get(0))
        .optional()?)
}

/// The list columns: no transcript/notes JSON leaves SQLite; the counts and the item id are
/// computed there (`speakers` that appear in the transcript, action items, those with a task).
const SUMMARY_COLS: &str = "id, title, source, device_id, started_at, duration_ms, language, langs, state, error, \
    summary_state, audio_deleted_at, \
    (SELECT COUNT(*) FROM json_each(meetings.speakers) sp WHERE json_extract(sp.value, '$.key') IN \
        (SELECT json_extract(seg.value, '$.speaker') FROM json_each(meetings.transcript) seg)), \
    json_array_length(actions), \
    (SELECT COUNT(*) FROM json_each(meetings.actions) a WHERE json_extract(a.value, '$.taskId') IS NOT NULL), \
    (SELECT i.id FROM items i WHERE i.meeting_id = meetings.id AND i.deleted_at IS NULL \
        ORDER BY i.captured_at LIMIT 1)";

fn map_summary(
    r: &Row<'_>,
    device_id: &str,
    paths: &Paths,
    progress: Option<(&str, f64)>,
) -> rusqlite::Result<(MeetingSummary, Option<String>)> {
    let id: String = r.get(0)?;
    let row_device: String = r.get(3)?;
    let audio_deleted_at: Option<i64> = r.get(11)?;
    let mix = paths.recording_dir(&id).join("mix.m4a");
    let audio = if audio_deleted_at.is_some() {
        "deleted"
    } else if row_device != device_id {
        "elsewhere"
    } else if mix.exists() {
        "available"
    } else {
        "processing"
    };
    let audio_path = (audio == "available").then(|| mix.to_string_lossy().into_owned());
    let summary = MeetingSummary {
        progress: progress.filter(|(p, _)| *p == id).map(|(_, p)| p),
        id,
        title: r.get(1)?,
        source: r.get(2)?,
        started_at: r.get(4)?,
        duration_ms: r.get(5)?,
        language: r.get(6)?,
        langs: json_col(r, 7)?,
        state: r.get(8)?,
        error: r.get(9)?,
        summary_state: r.get(10)?,
        speaker_count: r.get(12)?,
        action_count: r.get(13)?,
        tasks_added: r.get(14)?,
        item_id: r.get(15)?,
        on_this_device: row_device == device_id,
        audio: audio.into(),
    };
    Ok((summary, audio_path))
}

/// Live meetings matching `filter` (SQL after `AND`), newest first, at most 500, with their
/// audio paths.
fn summary_rows(
    conn: &Connection,
    filter: &str,
    values: &[&dyn ToSql],
    device_id: &str,
    paths: &Paths,
    progress: Option<(&str, f64)>,
) -> AppResult<Vec<(MeetingSummary, Option<String>)>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SUMMARY_COLS} FROM meetings WHERE deleted_at IS NULL AND {filter}
         ORDER BY started_at DESC, id DESC LIMIT {LIST_LIMIT}"
    ))?;
    let rows = stmt.query_map(values, |r| map_summary(r, device_id, paths, progress))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Live meetings, newest first, at most 500. `progress`: the transcribing meeting's id and
/// fraction done.
pub fn summaries(
    conn: &Connection,
    device_id: &str,
    paths: &Paths,
    progress: Option<(&str, f64)>,
) -> AppResult<Vec<MeetingSummary>> {
    let rows = summary_rows(conn, "1", &[], device_id, paths, progress)?;
    Ok(rows.into_iter().map(|(s, _)| s).collect())
}

pub fn detail(
    conn: &Connection,
    id: &str,
    device_id: &str,
    paths: &Paths,
    progress: Option<(&str, f64)>,
) -> AppResult<MeetingDetail> {
    let m = get(conn, id)?;
    let (meeting, audio_path) =
        summary_rows(conn, "id = ?1", &[&id], device_id, paths, progress)?.pop().ok_or(AppError::Code("notFound"))?;
    Ok(MeetingDetail {
        meeting,
        speakers: m.speakers,
        transcript: m.transcript,
        summary: m.summary,
        decisions: m.decisions,
        questions: m.questions,
        actions: m.actions,
        changes: m.changes,
        summary_lang: m.summary_lang,
        summary_error: m.summary_error,
        audio_path,
    })
}

// ---------- text ----------

/// The speaker's name, else its default label (`Me`, `Others`, `Speaker N`).
pub fn display_name(sp: &Speaker, lang: Lang) -> String {
    if let Some(name) = &sp.name {
        return name.clone();
    }
    match sp.key.as_str() {
        "me" => tr(lang, "meeting.me").into(),
        "others" => tr(lang, "meeting.others").into(),
        key => {
            let n: String = key.chars().filter(char::is_ascii_digit).collect();
            tr(lang, "meeting.speaker").replace("{n}", &n)
        }
    }
}

/// The display name of the speaker with `key` (a default label when it isn't listed).
pub(crate) fn speaker_name(m: &MeetingRow, key: &str, lang: Lang) -> String {
    match m.speakers.iter().find(|s| s.key == key) {
        Some(sp) => display_name(sp, lang),
        None => display_name(&Speaker { key: key.into(), name: None }, lang),
    }
}

/// Rounded minutes, at least 1.
pub fn duration_min(ms: i64) -> i64 {
    ((ms + 30_000) / 60_000).max(1)
}

/// `45 min`, `1 h 5 min` / `45 phút`, `1 giờ 5 phút` / `45 分钟`, `1 小时 5 分钟` (at least 1 minute).
fn duration_label(ms: i64, lang: Lang) -> String {
    let (min_unit, hour_unit) = match lang {
        Lang::En | Lang::Es => ("min", "h"),
        Lang::Vi => ("phút", "giờ"),
        Lang::Zh => ("分钟", "小时"),
    };
    let total = duration_min(ms);
    let (h, m) = (total / 60, total % 60);
    match (h, m) {
        (0, m) => format!("{m} {min_unit}"),
        (h, 0) => format!("{h} {hour_unit}"),
        (h, m) => format!("{h} {hour_unit} {m} {min_unit}"),
    }
}

/// The detected language codes, else the chosen ones (none for `auto`).
pub fn language_codes(m: &MeetingRow) -> Vec<&str> {
    if m.langs.is_empty() {
        m.language.split('+').filter(|c| *c != "auto").collect()
    } else {
        m.langs.iter().map(String::as_str).collect()
    }
}

/// `English + Vietnamese`: the detected languages, else the chosen one (empty for `auto`).
fn language_names(m: &MeetingRow, lang: Lang) -> String {
    language_codes(m)
        .into_iter()
        .filter_map(|c| match c {
            "en" => Some(tr(lang, "lang.en")),
            "vi" => Some(tr(lang, "lang.vi")),
            "zh" => Some(tr(lang, "lang.zh")),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

fn section(out: &mut String, heading: &str, lines: impl IntoIterator<Item = String>) {
    let lines: Vec<String> = lines.into_iter().collect();
    if lines.is_empty() {
        return;
    }
    out.push_str(&format!("\n## {heading}\n"));
    for l in lines {
        out.push_str(&l);
        out.push('\n');
    }
}

/// The meeting as Markdown (the Inbox item's text, copy and `.md` export). Empty sections are
/// omitted; the transcript only when `transcript` is set.
pub fn markdown(m: &MeetingRow, lang: Lang, transcript: bool) -> String {
    let started = Local.timestamp_millis_opt(m.started_at).single().unwrap_or_default();
    let mut meta = vec![
        short_date(lang, started.date_naive()),
        started.format("%H:%M").to_string(),
        duration_label(m.duration_ms, lang),
    ];
    let langs = language_names(m, lang);
    if !langs.is_empty() {
        meta.push(langs);
    }
    let mut out = format!("# {}\n{}\n", m.title, meta.join(" · "));
    let bullets = |v: &[String]| v.iter().map(|s| format!("- {s}")).collect::<Vec<_>>();
    section(&mut out, tr(lang, "meeting.summary"), bullets(&m.summary));
    section(&mut out, tr(lang, "meeting.decisions"), bullets(&m.decisions));
    section(
        &mut out,
        tr(lang, "meeting.actions"),
        m.actions.iter().map(|a| {
            let owner = match (&a.owner_key, &a.owner) {
                (Some(key), _) => Some(speaker_name(m, key, lang)),
                (None, owner) => owner.clone(),
            };
            let mut line = String::from("- [ ] ");
            if let Some(o) = owner {
                line.push_str(&format!("{o} — "));
            }
            line.push_str(&a.text);
            if let Some(d) = a.due.as_deref().and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()) {
                line.push_str(&format!(" ({} {})", tr(lang, "meeting.due"), short_date(lang, d)));
            }
            line
        }),
    );
    section(&mut out, tr(lang, "meeting.questions"), bullets(&m.questions));
    if transcript {
        section(
            &mut out,
            tr(lang, "meeting.transcript"),
            m.transcript.iter().map(|s| {
                let secs = s.start_ms.max(0) / 1000;
                format!("[{:02}:{:02}] {}: {}", secs / 60, secs % 60, speaker_name(m, &s.speaker, lang), s.text)
            }),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::items::{upsert_meeting_item, MeetingItem};
    use crate::db::Db;

    fn seg(speaker: &str, start_ms: i64, text: &str) -> TranscriptSeg {
        TranscriptSeg { start_ms, end_ms: start_ms + 1000, speaker: speaker.into(), text: text.into(), lang: "en".into() }
    }

    fn sp(key: &str, name: Option<&str>) -> Speaker {
        Speaker { key: key.into(), name: name.map(String::from) }
    }

    fn meeting(conn: &Connection, device: &str) -> String {
        create(conn, "recording", "en", device, 1_000, "Standup", None).unwrap()
    }

    fn item(meeting_id: &str, summary: &str) -> MeetingItem {
        MeetingItem {
            meeting_id: meeting_id.into(),
            title: "Standup".into(),
            summary: Some(summary.into()),
            payload: format!("# Standup\n- {summary}"),
            sensitive: false,
            captured_at: 1_000,
        }
    }

    #[test]
    fn rename_speaker_rejects_me_and_unknown() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        let id = meeting(&conn, "d1");
        save_transcript(&conn, &id, &[seg("me", 0, "hi")], &[sp("me", None), sp("s1", None)], &[], true).unwrap();
        assert!(matches!(rename_speaker(&conn, &id, "me", Some("Bob")), Err(AppError::Code("invalid"))));
        assert!(matches!(rename_speaker(&conn, &id, "s9", Some("Bob")), Err(AppError::Code("invalid"))));
        assert!(matches!(rename_speaker(&conn, &id, "s1", Some(&"x".repeat(61))), Err(AppError::Code("invalid"))));
        rename_speaker(&conn, &id, "s1", Some("  Lan  ")).unwrap();
        assert_eq!(get(&conn, &id).unwrap().speakers[1], sp("s1", Some("Lan")));
        rename_speaker(&conn, &id, "s1", Some("  ")).unwrap();
        assert_eq!(get(&conn, &id).unwrap().speakers[1], sp("s1", None));
    }

    #[test]
    fn markdown_omits_empty_sections_and_uses_renamed_owner() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        let id = meeting(&conn, "d1");
        save_transcript(
            &conn,
            &id,
            &[seg("me", 0, "Hello"), seg("s1", 65_000, "Ship it")],
            &[sp("me", None), sp("s1", Some("Lan"))],
            &["en".into()],
            true,
        )
        .unwrap();
        save_notes(&conn, &id, None, &["Agreed on scope".into()], &[], &[], "en").unwrap();
        let action = |owner: Option<&str>, owner_key: Option<&str>, due: Option<&str>, text: &str| ActionItem {
            id: new_id(),
            text: text.into(),
            owner: owner.map(String::from),
            owner_key: owner_key.map(String::from),
            due: due.map(String::from),
            at_ms: None,
            task_id: None,
        };
        save_actions(
            &conn,
            &id,
            &[
                action(Some("speaker 1"), Some("s1"), Some("2026-10-15"), "Send deck"),
                action(Some("Minh"), None, None, "Book room"),
                action(None, None, None, "Follow up"),
            ],
        )
        .unwrap();
        let m = get(&conn, &id).unwrap();
        let md = markdown(&m, Lang::En, false);
        assert!(md.starts_with("# Standup\n"));
        assert!(md.contains(" · English\n"));
        assert!(md.contains("## Summary\n- Agreed on scope\n"));
        assert!(md.contains("## Action items\n- [ ] Lan — Send deck (due Thu 15 Oct)\n- [ ] Minh — Book room\n- [ ] Follow up\n"));
        assert!(!md.contains("## Decisions"));
        assert!(!md.contains("## Open questions"));
        assert!(!md.contains("## Transcript"));
        let md = markdown(&m, Lang::Vi, true);
        assert!(md.contains("## Bản ghi\n[00:00] Tôi: Hello\n[01:05] Lan: Ship it\n"));
        assert!(md.contains("(hạn T5, 15/10)"));
    }

    #[test]
    fn meeting_item_upsert_updates_not_duplicates() {
        let db = Db::open_in_memory().unwrap();
        let mut conn = db.conn();
        let id = meeting(&conn, "d1");
        let first = upsert_meeting_item(&mut conn, &item(&id, "alpha")).unwrap();
        let second = upsert_meeting_item(&mut conn, &item(&id, "zebrafish")).unwrap();
        assert_eq!(first, second);
        assert_eq!(item_id(&conn, &id).unwrap(), Some(first.clone()));
        let (live, category, triage): (i64, String, String) = conn
            .query_row(
                "SELECT COUNT(*), MAX(category_id), MAX(triage_status) FROM items WHERE meeting_id = ?1 AND deleted_at IS NULL",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((live, category.as_str(), triage.as_str()), (1, "meeting", "done"));
        let hit: String = conn
            .query_row("SELECT item_id FROM items_fts WHERE items_fts MATCH '\"zebrafish\"*'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(hit, first);
        let stale: i64 = conn
            .query_row("SELECT COUNT(*) FROM items_fts WHERE items_fts MATCH '\"alpha\"*'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stale, 0);
    }

    #[test]
    fn trashed_meeting_item_is_not_resurrected() {
        let db = Db::open_in_memory().unwrap();
        let mut conn = db.conn();
        let id = meeting(&conn, "d1");
        let first = upsert_meeting_item(&mut conn, &item(&id, "alpha")).unwrap();
        conn.execute("UPDATE items SET deleted_at = 5 WHERE id = ?1", [&first]).unwrap();
        assert_eq!(upsert_meeting_item(&mut conn, &item(&id, "zebrafish")).unwrap(), first);
        let (rows, summary): (i64, String) = conn
            .query_row("SELECT COUNT(*), MAX(summary) FROM items WHERE meeting_id = ?1", [&id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((rows, summary.as_str()), (1, "alpha"));
        assert_eq!(item_id(&conn, &id).unwrap(), None);
    }

    #[test]
    fn meeting_item_falls_back_to_unsorted_when_meeting_category_deleted() {
        let db = Db::open_in_memory().unwrap();
        let mut conn = db.conn();
        conn.execute("UPDATE categories SET deleted_at = 1 WHERE id = 'meeting'", []).unwrap();
        let id = meeting(&conn, "d1");
        let item_id = upsert_meeting_item(&mut conn, &item(&id, "alpha")).unwrap();
        let category: String =
            conn.query_row("SELECT category_id FROM items WHERE id = ?1", [&item_id], |r| r.get(0)).unwrap();
        assert_eq!(category, "unsorted");
    }

    #[test]
    fn audio_state_rules() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        paths.ensure().unwrap();
        let audio = |id: &str| detail(&conn, id, "d1", &paths, None).unwrap();

        let processing = meeting(&conn, "d1");
        assert_eq!(audio(&processing).meeting.audio, "processing");
        assert_eq!(audio(&processing).audio_path, None);

        let available = meeting(&conn, "d1");
        std::fs::create_dir_all(paths.recording_dir(&available)).unwrap();
        let mix = paths.recording_dir(&available).join("mix.m4a");
        std::fs::write(&mix, b"aac").unwrap();
        let d = audio(&available);
        assert_eq!(d.meeting.audio, "available");
        assert_eq!(d.audio_path.as_deref(), Some(mix.to_string_lossy().as_ref()));

        let elsewhere = meeting(&conn, "d2");
        let d = audio(&elsewhere);
        assert_eq!((d.meeting.audio.as_str(), d.meeting.on_this_device), ("elsewhere", false));

        set_audio_deleted(&conn, &available, 5).unwrap();
        let deleted = meeting(&conn, "d2");
        set_audio_deleted(&conn, &deleted, 5).unwrap();
        assert_eq!(audio(&available).meeting.audio, "deleted");
        assert_eq!(audio(&deleted).meeting.audio, "deleted");

        let list = summaries(&conn, "d1", &paths, Some((processing.as_str(), 0.5))).unwrap();
        assert_eq!(list.len(), 4);
        let p = list.iter().find(|m| m.id == processing).unwrap();
        assert_eq!(p.progress, Some(0.5));
        assert!(list.iter().filter(|m| m.id != processing).all(|m| m.progress.is_none()));
    }

    #[test]
    fn summary_counts_and_item_come_from_sql() {
        let db = Db::open_in_memory().unwrap();
        let mut conn = db.conn();
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf());
        let id = meeting(&conn, "d1");
        save_transcript(
            &conn,
            &id,
            &[seg("me", 0, "a"), seg("s2", 1_000, "b"), seg("s2", 2_000, "c")],
            &[sp("me", None), sp("s1", None), sp("s2", Some("Lan"))],
            &[],
            true,
        )
        .unwrap();
        let action = |task_id: Option<&str>| ActionItem {
            id: new_id(),
            text: "x".into(),
            owner: None,
            owner_key: None,
            due: None,
            at_ms: None,
            task_id: task_id.map(String::from),
        };
        save_actions(&conn, &id, &[action(Some("t1")), action(None), action(Some("t2"))]).unwrap();
        let empty = meeting(&conn, "d1");
        let it = upsert_meeting_item(&mut conn, &item(&id, "alpha")).unwrap();

        let list = summaries(&conn, "d1", &paths, None).unwrap();
        let m = list.iter().find(|m| m.id == id).unwrap();
        assert_eq!((m.speaker_count, m.action_count, m.tasks_added), (2, 3, 2));
        assert_eq!(m.item_id.as_deref(), Some(it.as_str()));
        let e = list.iter().find(|m| m.id == empty).unwrap();
        assert_eq!((e.speaker_count, e.action_count, e.tasks_added, e.item_id.clone()), (0, 0, 0, None));
        assert_eq!(detail(&conn, &id, "d1", &paths, None).unwrap().meeting, *m);
        assert!(matches!(detail(&conn, "nope", "d1", &paths, None), Err(AppError::Code("notFound"))));
    }

    #[test]
    fn auto_title_never_overrides_user_or_ai() {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        let title = |id: &str| {
            let m = get(&conn, id).unwrap();
            (m.title, m.title_source)
        };

        let auto = meeting(&conn, "d1");
        set_auto_title(&conn, &auto, "Zoom call").unwrap();
        assert_eq!(title(&auto), ("Zoom call".into(), "auto".into()));
        save_notes(&conn, &auto, Some("Roadmap sync"), &[], &[], &[], "en").unwrap();
        assert_eq!(title(&auto), ("Roadmap sync".into(), "ai".into()), "the AI title still replaces it");
        set_auto_title(&conn, &auto, "Zoom call").unwrap();
        assert_eq!(title(&auto), ("Roadmap sync".into(), "ai".into()));

        let user = meeting(&conn, "d1");
        rename(&conn, &user, "Weekly 1:1").unwrap();
        set_auto_title(&conn, &user, "Zoom call").unwrap();
        assert_eq!(title(&user), ("Weekly 1:1".into(), "user".into()));
    }
}
