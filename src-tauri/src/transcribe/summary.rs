//! Meeting summaries: the summary job (privacy gates, notes from the selected AI provider, action items
//! to Tasks, proposed task changes) and the meeting commands acting on its results.
use std::collections::BTreeMap;
use std::future::Future;

use chrono::{Local, TimeZone};
use rusqlite::Connection;
use serde_json::Value;
use tauri::{AppHandle, Manager};

use super::extract::{self, Notes};
use super::worker::{self, item_state, write_item};
use crate::db::meetings::{self, speaker_name, MeetingRow};
use crate::db::search::fold;
use crate::db::{items, settings, tasks, Db};
use crate::error::{AppError, AppResult};
use crate::events::{self, PetReaction};
use crate::i18n::{self, short_date, tr, Lang};
use crate::ai::ChatOpts;
use crate::model::{ActionItem, Actor, NewTask, TaskChange, TaskPatch, TaskSummary};
use crate::platform::pasteboard::{self, TYPE_PLAIN};
use crate::triage::secrets;

/// Summary languages a user may ask for.
const SUMMARY_LANGS: [&str; 3] = ["en", "vi", "zh"];
const TASK_TITLE_MAX: usize = 300;

#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    /// The first summary: action items become Tasks.
    First,
    /// After a re-transcription: actions that became Tasks stay, new ones are only listed.
    Retranscribe,
    /// The user re-summarized in `lang`: only the notes change.
    NotesOnly { lang: String },
}

#[derive(Debug, PartialEq)]
pub(crate) enum Summarized {
    /// `skipped` with this error.
    Skipped(&'static str),
    /// `failed`.
    Failed,
    /// `done`: the meeting item and the tasks created.
    Done { item_id: String, created: Vec<String> },
}

/// The first gate that stops a summary, in order: AI off or local-only, a sensitive meeting item
/// or a secret in the transcript (the meeting item then becomes sensitive), the item's category
/// excluded from AI, no usable model: `noModel`, `notSetUp`, `cloudMeetingsOff` (`blocked`, from
/// `ai::route`).
fn gate(conn: &mut Connection, m: &MeetingRow, lang: Lang, blocked: Option<&'static str>) -> AppResult<Option<&'static str>> {
    if !settings::get::<bool>(conn, "ai.enabled")? || settings::get::<bool>(conn, "privacy.localOnly")? {
        return Ok(Some("aiOff"));
    }
    let item = item_state(conn, &m.id)?;
    if item.as_ref().is_some_and(|(_, _, sensitive)| *sensitive) {
        return Ok(Some("sensitive"));
    }
    let text = m.transcript.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("\n");
    if settings::get::<bool>(conn, "privacy.secretDetection")? && secrets::is_sensitive(&text) {
        write_item(conn, m, lang, item.and_then(|(_, summary, _)| summary), true)?;
        return Ok(Some("sensitive"));
    }
    let private: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM items i JOIN categories c ON c.id = i.category_id
           WHERE i.meeting_id = ?1 AND i.deleted_at IS NULL AND c.exclude_from_ai = 1)",
        [&m.id],
        |r| r.get(0),
    )?;
    if private {
        return Ok(Some("private"));
    }
    Ok(blocked)
}

/// The task for an action item: its text (at most 300 characters), a note naming the meeting
/// (and the owner, when there is one), its due date and the meeting item as source.
fn new_task(m: &MeetingRow, a: &ActionItem, lang: Lang, item: Option<String>) -> NewTask {
    let date = Local.timestamp_millis_opt(m.started_at).single().unwrap_or_default().date_naive();
    let mut notes = tr(lang, "meeting.taskFrom").replace("{title}", &m.title).replace("{date}", &short_date(lang, date));
    let owner = match (&a.owner_key, &a.owner) {
        (Some(key), _) => Some(speaker_name(m, key, lang)),
        (None, owner) => owner.clone(),
    };
    if let Some(owner) = owner {
        notes.push('\n');
        notes.push_str(&tr(lang, "meeting.owner").replace("{name}", &owner));
    }
    NewTask {
        title: a.text.chars().take(TASK_TITLE_MAX).collect(),
        notes: Some(notes),
        due_date: a.due.clone(),
        due_time: None,
        priority: None,
        estimate_min: None,
        first_step: None,
        source_item_id: item,
    }
}

/// Stores `notes` per `mode` and rewrites the meeting item. Returns the item id and the tasks
/// created.
fn save(conn: &mut Connection, id: &str, mode: &Mode, notes: Notes, code: &str, lang: Lang) -> AppResult<(String, Vec<String>)> {
    let title = Some(notes.title.as_str()).filter(|t| !t.is_empty());
    meetings::save_notes(conn, id, title, &notes.summary, &notes.decisions, &notes.questions, code)?;
    let m = meetings::get(conn, id)?;
    let mut created = Vec::new();
    let keep_settled = |changes: &[TaskChange]| -> Vec<TaskChange> {
        changes.iter().filter(|c| c.state != "pending").cloned().chain(notes.changes.iter().cloned()).collect()
    };
    match mode {
        Mode::First => {
            let item = meetings::item_id(conn, id)?;
            let mut actions = notes.actions.clone();
            for a in &mut actions {
                let (task, _) = tasks::create(conn, &new_task(&m, a, lang, item.clone()), &Actor::Meeting)?;
                a.task_id = Some(task.id.clone());
                created.push(task.id);
            }
            meetings::save_actions(conn, id, &actions)?;
            meetings::save_changes(conn, id, &keep_settled(&m.changes))?;
            meetings::mark_tasks_created(conn, id)?;
        }
        Mode::Retranscribe => {
            let mut actions: Vec<ActionItem> = m.actions.iter().filter(|a| a.task_id.is_some()).cloned().collect();
            for a in &notes.actions {
                if !actions.iter().any(|x| fold(&x.text) == fold(&a.text)) {
                    actions.push(a.clone());
                }
            }
            meetings::save_actions(conn, id, &actions)?;
            meetings::save_changes(conn, id, &keep_settled(&m.changes))?;
        }
        Mode::NotesOnly { .. } => {}
    }
    let m = meetings::get(conn, id)?;
    let sensitive = item_state(conn, id)?.is_some_and(|(_, _, sensitive)| sensitive);
    let summary = Some(m.summary.iter().take(2).cloned().collect::<Vec<_>>().join(" ")).filter(|s| !s.is_empty());
    let item_id = write_item(conn, &m, lang, summary, sensitive)?;
    Ok((item_id, created))
}

/// Summarizes meeting `id` in `mode`, asking `llm` (see [`extract::extract`]). `lang` is the UI
/// language (prompt speaker labels, Markdown, task notes); `started` runs once the meeting is
/// `running`. Model and save errors leave it `failed`.
pub(crate) async fn summarize_with<F, Fut>(
    db: &Db,
    lang: Lang,
    id: &str,
    mode: Mode,
    blocked: Option<&'static str>,
    started: &(dyn Fn() + Sync),
    llm: F,
) -> AppResult<Summarized>
where
    F: FnMut(&'static ChatOpts, String, String, Value) -> Fut,
    Fut: Future<Output = AppResult<Value>>,
{
    let (m, tasks, code) = {
        let mut conn = db.conn();
        let m = meetings::get(&conn, id)?;
        if let Some(code) = gate(&mut conn, &m, lang, blocked)? {
            meetings::set_summary_state(&conn, id, "skipped", Some(code))?;
            return Ok(Summarized::Skipped(code));
        }
        meetings::set_summary_state(&conn, id, "running", None)?;
        let (tasks, code) = match &mode {
            Mode::NotesOnly { lang } => (Vec::new(), extract::target_lang(lang, &m, Lang::En)),
            _ => {
                let setting: String = settings::get(&conn, "transcribe.summaryLanguage")?;
                (tasks::open_for_meeting(&conn)?, extract::target_lang(&setting, &m, lang))
            }
        };
        (m, tasks, code)
    };
    started();
    let with_actions = !matches!(mode, Mode::NotesOnly { .. });
    let lines = extract::lines(&m, lang);
    let notes = extract::extract(&m, &lines, &tasks, extract::lang_name(code), with_actions, llm).await;
    let mut conn = db.conn();
    let saved = notes.and_then(|notes| save(&mut conn, id, &mode, notes, code, lang));
    match saved {
        Ok((item_id, created)) => {
            meetings::set_summary_state(&conn, id, "done", None)?;
            Ok(Summarized::Done { item_id, created })
        }
        Err(e) => {
            eprintln!("[transcribe] summary {id} failed: {e}");
            meetings::set_summary_state(&conn, id, "failed", Some("failed"))?;
            Ok(Summarized::Failed)
        }
    }
}

/// The worker's mode for a pending meeting: `First` until its action items became tasks.
fn pending_mode(m: &MeetingRow) -> Mode {
    if m.tasks_created_at.is_none() {
        Mode::First
    } else {
        Mode::Retranscribe
    }
}

/// [`summarize_with`] with the selected AI provider, then the notifications. `Err` only when the
/// meeting's summary state could not be recorded.
async fn summarize(app: &AppHandle, id: &str, mode: Mode) -> AppResult<()> {
    let first = mode == Mode::First;
    let lang = i18n::lang(app);
    let blocked = crate::ai::route(app, crate::ai::Purpose::Meeting).err();
    let notify_running = || meetings::notify_changed(app, vec![id.to_string()]);
    let result = summarize_with(app.state::<Db>().inner(), lang, id, mode, blocked, &notify_running, |opts, sys, user, schema| async move {
        crate::ai::chat_json(app, crate::ai::Purpose::Meeting, opts, &sys, &user, &schema).await
    })
    .await?;
    meetings::notify_changed(app, vec![id.to_string()]);
    match result {
        Summarized::Done { item_id, created } => {
            items::notify_changed(app, vec![item_id]);
            if !created.is_empty() {
                tasks::notify_changed(app, created);
                if first {
                    crate::next::after_meeting(app);
                }
            }
            events::pet_reaction(app, PetReaction { category: Some("meeting".into()), ..PetReaction::new("idea") });
        }
        Summarized::Skipped("sensitive") => {
            if let Some(item) = meetings::item_id(&app.state::<Db>().conn(), id)? {
                items::notify_changed(app, vec![item]);
            }
        }
        Summarized::Skipped(_) | Summarized::Failed => {}
    }
    Ok(())
}

/// The worker's summary job for a `pending` meeting.
pub(super) async fn run_pending(app: &AppHandle, id: &str) -> AppResult<()> {
    let m = meetings::get(&app.state::<Db>().conn(), id)?;
    summarize(app, id, pending_mode(&m)).await
}

// ---------- meeting commands ----------

/// Summarizes a `done` meeting again in `lang` (`en|vi|zh`), changing only its notes. Errors:
/// `busy` while a summary runs, `invalid` otherwise.
pub async fn resummarize(app: &AppHandle, id: &str, lang: &str) -> AppResult<()> {
    if !SUMMARY_LANGS.contains(&lang) {
        return Err(AppError::Code("invalid"));
    }
    match meetings::get(&app.state::<Db>().conn(), id)?.summary_state.as_str() {
        "running" => return Err(AppError::Code("busy")),
        "done" => {}
        _ => return Err(AppError::Code("invalid")),
    }
    summarize(app, id, Mode::NotesOnly { lang: lang.to_owned() }).await
}

/// Queues a `skipped` or `failed` summary again. Other states are `invalid`.
pub fn summarize_again(app: &AppHandle, id: &str) -> AppResult<()> {
    {
        let conn = app.state::<Db>().inner().conn();
        if !matches!(meetings::get(&conn, id)?.summary_state.as_str(), "skipped" | "failed") {
            return Err(AppError::Code("invalid"));
        }
        meetings::set_summary_state(&conn, id, "pending", None)?;
    }
    meetings::notify_changed(app, vec![id.to_string()]);
    worker::poke(app);
    Ok(())
}

/// The meeting's changes and the index of `change_id` among them (`notFound` when missing).
fn find_change(conn: &Connection, meeting_id: &str, change_id: &str) -> AppResult<(Vec<TaskChange>, usize)> {
    let changes = meetings::get(conn, meeting_id)?.changes;
    let i = changes.iter().position(|c| c.id == change_id).ok_or(AppError::Code("notFound"))?;
    Ok((changes, i))
}

/// Applies a pending change to its live task as the user; returns the change (now `applied`
/// with its event ids). `notFound` for a missing change or task, `invalid` unless pending.
fn apply_change(conn: &mut Connection, meeting_id: &str, change_id: &str) -> AppResult<TaskChange> {
    let (mut changes, i) = find_change(conn, meeting_id, change_id)?;
    let c = &mut changes[i];
    if c.state != "pending" {
        return Err(AppError::Code("invalid"));
    }
    tasks::summary(conn, &c.task_id)?;
    let to = || c.to.clone().ok_or(AppError::Code("invalid"));
    let event_ids = match c.field.as_str() {
        "due" => tasks::update(conn, &c.task_id, &TaskPatch { due_date: Some(Some(to()?)), ..Default::default() }, &Actor::User)?,
        "title" => tasks::update(conn, &c.task_id, &TaskPatch { title: Some(to()?), ..Default::default() }, &Actor::User)?,
        "done" => tasks::set_done(conn, &c.task_id, true, &Actor::User)?,
        _ => return Err(AppError::Code("invalid")),
    };
    c.state = "applied".into();
    c.event_ids = event_ids;
    let applied = c.clone();
    meetings::save_changes(conn, meeting_id, &changes)?;
    Ok(applied)
}

/// Undoes an applied change's task events (already undone ones are fine) and makes it pending
/// again. Returns its task id. `invalid` unless applied.
fn undo_change(conn: &mut Connection, meeting_id: &str, change_id: &str) -> AppResult<String> {
    let (mut changes, i) = find_change(conn, meeting_id, change_id)?;
    let c = &mut changes[i];
    if c.state != "applied" {
        return Err(AppError::Code("invalid"));
    }
    for event in &c.event_ids {
        match tasks::undo_event(conn, event) {
            Ok(_) | Err(AppError::Code("alreadyUndone")) => {}
            Err(e) => return Err(e),
        }
    }
    c.state = "pending".into();
    c.event_ids.clear();
    let task_id = c.task_id.clone();
    meetings::save_changes(conn, meeting_id, &changes)?;
    Ok(task_id)
}

/// Marks a pending change `ignored`. `invalid` unless pending.
fn ignore_change(conn: &Connection, meeting_id: &str, change_id: &str) -> AppResult<()> {
    let (mut changes, i) = find_change(conn, meeting_id, change_id)?;
    if changes[i].state != "pending" {
        return Err(AppError::Code("invalid"));
    }
    changes[i].state = "ignored".into();
    meetings::save_changes(conn, meeting_id, &changes)
}

/// Makes an action item that has no task a task (as the user). `notFound` for a missing
/// action, `invalid` when it already has a task.
fn add_action(conn: &mut Connection, meeting_id: &str, action_id: &str, lang: Lang) -> AppResult<TaskSummary> {
    let m = meetings::get(conn, meeting_id)?;
    let mut actions = m.actions.clone();
    let a = actions.iter_mut().find(|a| a.id == action_id).ok_or(AppError::Code("notFound"))?;
    if a.task_id.is_some() {
        return Err(AppError::Code("invalid"));
    }
    let item = meetings::item_id(conn, meeting_id)?;
    let (task, _) = tasks::create(conn, &new_task(&m, a, lang, item), &Actor::User)?;
    a.task_id = Some(task.id.clone());
    meetings::save_actions(conn, meeting_id, &actions)?;
    Ok(task)
}

pub fn apply_meeting_change(app: &AppHandle, meeting_id: &str, change_id: &str) -> AppResult<Vec<String>> {
    let c = apply_change(&mut app.state::<Db>().conn(), meeting_id, change_id)?;
    meetings::notify_changed(app, vec![meeting_id.to_string()]);
    tasks::notify_changed(app, vec![c.task_id]);
    if c.field == "done" && !c.event_ids.is_empty() {
        events::celebrate(app);
    }
    Ok(c.event_ids)
}

pub fn undo_meeting_change(app: &AppHandle, meeting_id: &str, change_id: &str) -> AppResult<()> {
    let task_id = undo_change(&mut app.state::<Db>().conn(), meeting_id, change_id)?;
    meetings::notify_changed(app, vec![meeting_id.to_string()]);
    tasks::notify_changed(app, vec![task_id]);
    Ok(())
}

pub fn ignore_meeting_change(app: &AppHandle, meeting_id: &str, change_id: &str) -> AppResult<()> {
    ignore_change(&app.state::<Db>().conn(), meeting_id, change_id)?;
    meetings::notify_changed(app, vec![meeting_id.to_string()]);
    Ok(())
}

pub fn add_action_to_tasks(app: &AppHandle, meeting_id: &str, action_id: &str) -> AppResult<TaskSummary> {
    // Before taking the connection: `lang` reads a setting through the same mutex.
    let lang = i18n::lang(app);
    let task = add_action(&mut app.state::<Db>().conn(), meeting_id, action_id, lang)?;
    meetings::notify_changed(app, vec![meeting_id.to_string()]);
    tasks::notify_changed(app, vec![task.id.clone()]);
    Ok(task)
}

/// Copies the meeting's notes (Markdown without the transcript) as plain text.
pub async fn copy_markdown(app: &AppHandle, id: &str) -> AppResult<()> {
    // Before taking the connection: `lang` reads a setting through the same mutex.
    let lang = i18n::lang(app);
    let text = meetings::markdown(&meetings::get(&app.state::<Db>().conn(), id)?, lang, false);
    pasteboard::write_on_main(app, Vec::new(), BTreeMap::from([(TYPE_PLAIN.to_string(), text)]), None).await
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::future::{ready, Ready};

    use serde_json::json;

    use super::*;
    use crate::export;
    use crate::model::{Speaker, TranscriptSeg};

    fn db() -> Db {
        Db::open_in_memory().unwrap()
    }

    /// A transcribed meeting with its item, summary `pending`.
    fn meeting(db: &Db, texts: &[&str]) -> String {
        let mut conn = db.conn();
        let device = export::device_id(&conn).unwrap();
        let id = meetings::create(&conn, "recording", "en", &device, 1_000, "Meeting", None).unwrap();
        let segs: Vec<TranscriptSeg> = texts
            .iter()
            .enumerate()
            .map(|(i, t)| TranscriptSeg {
                start_ms: i as i64 * 5000,
                end_ms: i as i64 * 5000 + 4000,
                speaker: "s1".into(),
                text: (*t).into(),
                lang: "en".into(),
            })
            .collect();
        let speakers = vec![Speaker { key: "s1".into(), name: Some("Huy".into()) }];
        meetings::save_transcript(&conn, &id, &segs, &speakers, &["en".into()], true).unwrap();
        meetings::set_state(&conn, &id, "done", None).unwrap();
        meetings::set_summary_state(&conn, &id, "pending", None).unwrap();
        let m = meetings::get(&conn, &id).unwrap();
        write_item(&mut conn, &m, Lang::En, None, false).unwrap();
        id
    }

    fn notes(actions: &[&str]) -> Value {
        let actions: Vec<Value> =
            actions.iter().map(|t| json!({ "text": t, "owner": "Huy", "due": "2026-10-23", "at": "0:05" })).collect();
        json!({
            "title": "Slack fallback",
            "summary": ["One.", "Two.", "Three."],
            "decisions": ["Ship it"],
            "questions": [],
            "actions": actions,
            "changes": [],
        })
    }

    type Schemas = Rc<RefCell<Vec<Value>>>;
    type Fake = Box<dyn FnMut(&'static ChatOpts, String, String, Value) -> Ready<AppResult<Value>>>;

    /// Answers every call with `answer`, recording each schema.
    fn fake(answer: Value, schemas: Rc<RefCell<Vec<Value>>>) -> Fake {
        Box::new(move |_, _, _, schema| {
            schemas.borrow_mut().push(schema);
            ready(Ok(answer.clone()))
        })
    }

    fn no_llm() -> Fake {
        Box::new(|_, _, _, _| panic!("the model must not be called"))
    }

    fn run(db: &Db, id: &str, mode: Mode, blocked: Option<&'static str>, llm: Fake) -> Summarized {
        tauri::async_runtime::block_on(summarize_with(db, Lang::En, id, mode, blocked, &|| {}, llm)).unwrap()
    }

    fn pending(db: &Db, id: &str) -> Summarized {
        let m = meetings::get(&db.conn(), id).unwrap();
        run(db, id, pending_mode(&m), None, fake(notes(&["Add the Slack fallback", "Write docs"]), Schemas::default()))
    }

    fn task_count(db: &Db) -> i64 {
        db.conn().query_row("SELECT COUNT(*) FROM tasks WHERE deleted_at IS NULL", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn first_summary_creates_one_task_per_action_once() {
        let db = db();
        let id = meeting(&db, &["Huy sẽ thêm fallback cho Slack trước thứ Sáu."]);
        let Summarized::Done { item_id, created } = pending(&db, &id) else { panic!() };
        assert_eq!(created.len(), 2);
        let m = meetings::get(&db.conn(), &id).unwrap();
        assert_eq!((m.summary_state.as_str(), m.title.as_str(), m.title_source.as_str()), ("done", "Slack fallback", "ai"));
        assert!(m.tasks_created_at.is_some());
        assert_eq!(m.actions.iter().filter_map(|a| a.task_id.clone()).collect::<Vec<_>>(), created);
        let task = tasks::get(&db.conn(), &created[0]).unwrap();
        assert_eq!(task.task.title, "Add the Slack fallback");
        assert_eq!(task.task.due_date.as_deref(), Some("2026-10-23"));
        assert_eq!(task.task.source_item_id.as_deref(), Some(item_id.as_str()));
        assert_eq!(task.task.source_meeting_id.as_deref(), Some(id.as_str()));
        assert!(task.notes.starts_with("From “Slack fallback” · "), "{}", task.notes);
        assert!(task.notes.ends_with("\nOwner: Huy"), "{}", task.notes);
        assert_eq!(task.events[0].actor, "meeting");
        let summary: Option<String> =
            db.conn().query_row("SELECT summary FROM items WHERE id = ?1", [&item_id], |r| r.get(0)).unwrap();
        assert_eq!(summary.as_deref(), Some("One. Two."));

        // A re-transcription queues the summary again: same actions plus a new one.
        meetings::set_summary_state(&db.conn(), &id, "pending", None).unwrap();
        let m = meetings::get(&db.conn(), &id).unwrap();
        assert_eq!(pending_mode(&m), Mode::Retranscribe);
        let answer = notes(&["add the SLACK fallback ", "Write docs", "Book a room"]);
        let Summarized::Done { created: again, .. } = run(&db, &id, pending_mode(&m), None, fake(answer, Schemas::default())) else {
            panic!()
        };
        assert!(again.is_empty());
        assert_eq!(task_count(&db), 2, "no duplicate tasks");
        let m = meetings::get(&db.conn(), &id).unwrap();
        let actions: Vec<(&str, bool)> = m.actions.iter().map(|a| (a.text.as_str(), a.task_id.is_some())).collect();
        assert_eq!(actions, [("Add the Slack fallback", true), ("Write docs", true), ("Book a room", false)]);
    }

    #[test]
    fn gates_skip_private_and_sensitive() {
        let db = db();
        let state = |id: &str| {
            let m = meetings::get(&db.conn(), id).unwrap();
            (m.summary_state, m.summary_error)
        };
        let skipped = |code: &str| ("skipped".to_string(), Some(code.to_string()));
        let item_sensitive = |id: &str| -> bool {
            let item = meetings::item_id(&db.conn(), id).unwrap().unwrap();
            db.conn().query_row("SELECT sensitive FROM items WHERE id = ?1", [&item], |r| r.get(0)).unwrap()
        };

        let secret = format!("deploy with sk-ant-api03-{}", "a".repeat(40));
        let id = meeting(&db, &["Hello.", &secret]);
        assert_eq!(run(&db, &id, Mode::First, Some("notSetUp"), no_llm()), Summarized::Skipped("sensitive"));
        assert_eq!(state(&id), skipped("sensitive"));
        assert!(item_sensitive(&id));

        // The item stays sensitive once detection is off: never sent to the model, in any mode.
        settings::put(&db.conn(), "privacy.secretDetection", &json!(false)).unwrap();
        for mode in [Mode::First, Mode::Retranscribe, Mode::NotesOnly { lang: "en".into() }] {
            meetings::set_summary_state(&db.conn(), &id, "pending", None).unwrap();
            assert_eq!(run(&db, &id, mode, Some("notSetUp"), no_llm()), Summarized::Skipped("sensitive"));
            assert_eq!(state(&id), skipped("sensitive"));
        }
        settings::put(&db.conn(), "privacy.secretDetection", &json!(true)).unwrap();

        let id = meeting(&db, &["Plain talk."]);
        let item = meetings::item_id(&db.conn(), &id).unwrap().unwrap();
        db.conn()
            .execute(
                "UPDATE categories SET exclude_from_ai = 1 WHERE id = (SELECT category_id FROM items WHERE id = ?1)",
                [&item],
            )
            .unwrap();
        assert_eq!(run(&db, &id, Mode::First, Some("notSetUp"), no_llm()), Summarized::Skipped("private"));
        assert_eq!(state(&id), skipped("private"));
        db.conn().execute("UPDATE categories SET exclude_from_ai = 0", []).unwrap();

        assert_eq!(run(&db, &id, Mode::First, Some("noModel"), no_llm()), Summarized::Skipped("noModel"));
        settings::put(&db.conn(), "privacy.localOnly", &json!(true)).unwrap();
        assert_eq!(run(&db, &id, Mode::First, None, no_llm()), Summarized::Skipped("aiOff"));
        assert_eq!(state(&id), skipped("aiOff"));
        assert_eq!(task_count(&db), 0);

        // AI off comes first: a secret transcript doesn't mark the item sensitive then.
        let id = meeting(&db, &[&secret]);
        assert_eq!(run(&db, &id, Mode::First, None, no_llm()), Summarized::Skipped("aiOff"));
        assert_eq!(state(&id), skipped("aiOff"));
        assert!(!item_sensitive(&id));
    }

    #[test]
    fn cloud_meetings_off_skips_without_calling() {
        let db = db();
        let id = meeting(&db, &["Plain talk."]);
        assert_eq!(run(&db, &id, Mode::First, Some("cloudMeetingsOff"), no_llm()), Summarized::Skipped("cloudMeetingsOff"));
        let m = meetings::get(&db.conn(), &id).unwrap();
        assert_eq!((m.summary_state.as_str(), m.summary_error.as_deref()), ("skipped", Some("cloudMeetingsOff")));
        assert_eq!(task_count(&db), 0);
    }

    #[test]
    fn apply_due_change_then_undo_restores_due() {
        let db = db();
        let id = meeting(&db, &["Move the API launch to Friday."]);
        let new = |title: &str| NewTask {
            title: title.into(),
            notes: None,
            due_date: Some("2026-10-20".into()),
            due_time: None,
            priority: None,
            estimate_min: None,
            first_step: None,
            source_item_id: None,
        };
        let (task, _) = tasks::create(&mut db.conn(), &new("Ship API"), &Actor::User).unwrap();
        let change = |cid: &str, field: &str, to: Option<&str>| TaskChange {
            id: cid.into(),
            task_id: task.id.clone(),
            field: field.into(),
            from: Some("2026-10-20".into()),
            to: to.map(String::from),
            at_ms: None,
            state: "pending".into(),
            event_ids: Vec::new(),
        };
        meetings::save_changes(&db.conn(), &id, &[change("c1", "due", Some("2026-10-23")), change("c2", "done", None)])
            .unwrap();
        let mut conn = db.conn();

        let applied = apply_change(&mut conn, &id, "c1").unwrap();
        assert_eq!((applied.state.as_str(), applied.event_ids.len()), ("applied", 1));
        assert_eq!(tasks::summary(&conn, &task.id).unwrap().due_date.as_deref(), Some("2026-10-23"));
        assert!(matches!(apply_change(&mut conn, &id, "c1"), Err(AppError::Code("invalid"))));
        let ev = tasks::get(&conn, &task.id).unwrap().events;
        assert_eq!(ev[0].actor, "user");

        assert_eq!(undo_change(&mut conn, &id, "c1").unwrap(), task.id);
        assert_eq!(tasks::summary(&conn, &task.id).unwrap().due_date.as_deref(), Some("2026-10-20"));
        let c = &meetings::get(&conn, &id).unwrap().changes[0];
        assert_eq!((c.state.as_str(), c.event_ids.len()), ("pending", 0));

        ignore_change(&conn, &id, "c2").unwrap();
        assert_eq!(meetings::get(&conn, &id).unwrap().changes[1].state, "ignored");
        assert!(matches!(apply_change(&mut conn, &id, "missing"), Err(AppError::Code("notFound"))));
        tasks::trash(&mut conn, &task.id, &Actor::User).unwrap();
        assert!(matches!(apply_change(&mut conn, &id, "c1"), Err(AppError::Code("notFound"))));
    }

    #[test]
    fn notes_only_keeps_actions() {
        let db = db();
        let id = meeting(&db, &["Huy will add the Slack fallback."]);
        pending(&db, &id);
        let before = meetings::get(&db.conn(), &id).unwrap();
        let schemas = Schemas::default();
        let answer = json!({ "title": "Dự phòng Slack", "summary": ["Một."], "decisions": [], "questions": [] });
        let lang = Mode::NotesOnly { lang: "vi".into() };
        assert!(matches!(run(&db, &id, lang, None, fake(answer, schemas.clone())), Summarized::Done { ref created, .. } if created.is_empty()));
        assert!(schemas.borrow()[0]["properties"].get("actions").is_none());
        let after = meetings::get(&db.conn(), &id).unwrap();
        assert_eq!(after.actions, before.actions);
        assert_eq!(after.changes, before.changes);
        assert_eq!(after.summary, ["Một."]);
        assert_eq!(after.summary_lang.as_deref(), Some("vi"));
        assert_eq!(after.title, "Slack fallback", "the AI title is replaced only while automatic");
        assert_eq!(task_count(&db), 2);

        let action = &after.actions[0].id;
        assert!(matches!(add_action(&mut db.conn(), &id, action, Lang::En), Err(AppError::Code("invalid"))));
    }
}
