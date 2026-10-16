//! Triage: secrets guard, deterministic rules and the local-model classifier, run by one
//! background worker that sorts every captured item in capture order.
pub mod ai;
pub mod rules;
pub mod secrets;

use std::future::Future;

use rusqlite::Connection;
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc::{self, error::TryRecvError, UnboundedReceiver, UnboundedSender};

use crate::db::{items, settings, Db};
use crate::error::{AppError, AppResult};
use crate::events::{self, PetFlags, PetReaction};
use crate::extract::{self, ExtractOut};
use crate::llm;
use crate::model::{ItemRow, Rule, TriageUpdate};
use crate::paths::Paths;
use ai::TriageOut;

/// Correction examples shown to the model.
const EXAMPLES: usize = 8;

/// Managed state: hands item ids to the triage worker.
pub struct TriageQueue {
    tx: UnboundedSender<String>,
}

impl TriageQueue {
    pub fn enqueue(&self, id: String) {
        if self.tx.send(id).is_err() {
            eprintln!("[triage] worker stopped; the item stays pending until the next launch");
        }
    }
}

/// Spawns the worker and queues every item still waiting for triage.
pub fn start(app: &AppHandle) -> TriageQueue {
    let (tx, rx) = mpsc::unbounded_channel();
    match items::pending_ids(&app.state::<Db>().conn()) {
        Ok(ids) => {
            for id in ids {
                let _ = tx.send(id);
            }
        }
        Err(e) => eprintln!("[triage] reading pending items failed: {e}"),
    }
    tauri::async_runtime::spawn(run(app.clone(), rx));
    TriageQueue { tx }
}

/// One item at a time. `pet-flags {thinking}` turns on when work arrives and off once the
/// queue is drained; only the worker flips it, so the transitions never race.
async fn run(app: AppHandle, mut rx: UnboundedReceiver<String>) {
    let mut thinking = false;
    loop {
        let id = match rx.try_recv() {
            Ok(id) => id,
            Err(TryRecvError::Disconnected) => return,
            Err(TryRecvError::Empty) => {
                if thinking {
                    thinking = false;
                    set_thinking(&app, false);
                }
                match rx.recv().await {
                    Some(id) => id,
                    None => return,
                }
            }
        };
        if !thinking {
            thinking = true;
            set_thinking(&app, true);
        }
        if let Err(e) = triage_item(&app, &id).await {
            eprintln!("[triage] {id} failed: {e}");
        }
    }
}

fn set_thinking(app: &AppHandle, on: bool) {
    events::pet_flags(app, PetFlags { thinking: Some(on), ..Default::default() });
}

/// Settings that steer one triage pass.
#[derive(Debug, Clone)]
pub(crate) struct Policy {
    pub secret_detection: bool,
    pub ai_enabled: bool,
    pub local_only: bool,
    /// A verified model file for `ai.modelId` exists.
    pub model_ready: bool,
    pub threshold: f64,
}

impl Policy {
    fn read(conn: &Connection) -> AppResult<Self> {
        Ok(Policy {
            secret_detection: settings::get(conn, "privacy.secretDetection")?,
            ai_enabled: settings::get(conn, "ai.enabled")?,
            local_only: settings::get(conn, "privacy.localOnly")?,
            model_ready: false,
            threshold: settings::get(conn, "triage.confidenceThreshold")?,
        })
    }
}

/// Where an item goes and why, before it is written back.
#[derive(Debug)]
pub(crate) struct Decision {
    pub category_id: String,
    /// `rule | ai | none`
    pub source: &'static str,
    pub confidence: Option<f64>,
    pub reason: String,
    pub suggestions: Vec<String>,
    pub sensitive: bool,
    /// The model's answer, when it was asked and replied: its title, summary and tags are kept.
    pub ai: Option<TriageOut>,
}

impl Decision {
    fn unsorted(reason: &'static str, sensitive: bool) -> Self {
        Decision {
            category_id: "unsorted".into(),
            source: "none",
            confidence: None,
            reason: reason.into(),
            suggestions: Vec::new(),
            sensitive,
            ai: None,
        }
    }
}

/// Folds the extraction result into the row, so the secrets guard, rules and the model all see
/// the extracted text, and returns the extraction status.
fn merge_extract(item: &mut ItemRow, ex: ExtractOut) -> &'static str {
    if ex.text.is_some() {
        item.extracted_text = ex.text;
    }
    // Only link previews produce a title (og:title or <title>).
    if let Some(title) = ex.title.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()) {
        item.title = title;
    }
    if ex.thumb.is_some() {
        item.thumb_path = ex.thumb;
    }
    ex.status
}

/// Plan Step 9.5, per item: secrets guard, then rules, then the local model when it is allowed,
/// enabled and downloaded. `classify` is called at most once, and never for sensitive items.
pub(crate) async fn decide<F, Fut>(item: &ItemRow, rules: &[Rule], p: &Policy, classify: F) -> Decision
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = AppResult<TriageOut>>,
{
    let sensitive = p.secret_detection && secrets::is_sensitive_item(item);
    if let Some(m) = rules::match_rules(item, rules) {
        return Decision {
            category_id: m.category_id,
            source: "rule",
            confidence: Some(1.0),
            reason: m.reason,
            suggestions: Vec::new(),
            sensitive,
            ai: None,
        };
    }
    if sensitive {
        return Decision::unsorted("none:sensitive", true);
    }
    if !p.ai_enabled || p.local_only {
        return Decision::unsorted("none:ai-off", false);
    }
    if !p.model_ready {
        return Decision::unsorted("none:no-model", false);
    }
    match classify().await {
        Ok(out) => {
            let (category_id, suggestions) = if out.confidence >= p.threshold && out.category != "unsorted" {
                (out.category.clone(), Vec::new())
            } else {
                let mut picks: Vec<String> = Vec::with_capacity(2);
                for c in [&out.category, &out.alternative] {
                    if c != "unsorted" && !picks.contains(c) {
                        picks.push(c.clone());
                    }
                }
                ("unsorted".to_owned(), picks)
            };
            Decision {
                category_id,
                source: "ai",
                confidence: Some(out.confidence),
                reason: "ai".into(),
                suggestions,
                sensitive: false,
                ai: Some(out),
            }
        }
        Err(e) => {
            eprintln!("[triage] {}: local model failed: {e}", item.id);
            Decision::unsorted("none:ai-error", false)
        }
    }
}

/// The row write for a decision. The model's title replaces the current one only for kinds
/// whose title is not a file name.
pub(crate) fn into_update(item: ItemRow, extract_status: &'static str, d: Decision) -> TriageUpdate {
    let mut title = item.title;
    let (mut summary, mut tags, mut task_suggestion) = (None, None, None);
    if let Some(out) = d.ai {
        let ai_titled = matches!(item.kind.as_str(), "text" | "richtext" | "code" | "url" | "screenshot");
        if ai_titled && !out.title.trim().is_empty() {
            title = out.title.trim().to_owned();
        }
        summary = Some(out.summary.trim().to_owned()).filter(|s| !s.is_empty());
        tags = Some(out.tags);
        if d.source == "ai" {
            task_suggestion = Some(out.task.trim().to_owned()).filter(|t| !t.is_empty());
        }
    }
    TriageUpdate {
        title: Some(title),
        summary,
        tags,
        extracted_text: item.extracted_text,
        extract_status: extract_status.into(),
        thumb_path: item.thumb_path,
        category_id: d.category_id,
        triage_source: d.source.into(),
        triage_confidence: d.confidence,
        triage_reason: d.reason,
        suggestions: d.suggestions,
        sensitive: d.sensitive,
        task_suggestion,
    }
}

async fn triage_item(app: &AppHandle, id: &str) -> AppResult<()> {
    let db = app.state::<Db>();
    let (mut item, rules, mut policy) = {
        let conn = db.conn();
        let item = items::get_row(&conn, id)?;
        if item.triage_status != "pending" || item.deleted_at.is_some() {
            return Ok(());
        }
        (item, crate::db::rules::list_active(&conn)?, Policy::read(&conn)?)
    };
    let categories = crate::db::categories::list(&db.conn())?;
    policy.model_ready = llm::active_model_path(app).is_some();
    let paths = app.state::<Paths>().inner().clone();
    let ex = extract::extract(&paths, &item, policy.local_only).await;
    let extract_status = merge_extract(&mut item, ex);

    let decision = decide(&item, &rules, &policy, || async {
        let examples = crate::db::rules::recent_examples(&db.conn(), EXAMPLES)?;
        ai::classify(app, &item, &examples, &categories).await
    })
    .await;
    let reaction = match decision.category_id.as_str() {
        "unsorted" => PetReaction::new("tilt"),
        category => PetReaction { category: Some(category.to_owned()), ..PetReaction::new("idea") },
    };
    let update = into_update(item, extract_status, decision);
    {
        let mut conn = db.conn();
        // A category the user picked while this item was being sorted wins.
        if items::get_row(&conn, id)?.triage_status != "pending" {
            return Ok(());
        }
        items::update_triage(&mut conn, id, &update)?;
    }
    items::notify_changed(app, vec![id.to_owned()]);
    events::pet_reaction(app, reaction);
    Ok(())
}

/// Plan Step 9.6 "Make it a rule": a user rule (position 0) sending this item's domain (urls)
/// or extension (files, images) to the item's current category. Other kinds: `notRuleable`.
pub fn make_rule_from_item(conn: &mut Connection, item_id: &str) -> AppResult<Rule> {
    let item = items::get_row(conn, item_id)?;
    let (kind, pattern) = match item.kind.as_str() {
        "url" => ("domain", item.url.as_deref().and_then(rules::host)),
        "file" | "image" => ("extension", item.file_name.as_deref().and_then(rules::extension)),
        _ => return Err(AppError::Code("notRuleable")),
    };
    let pattern = pattern.ok_or(AppError::Code("notRuleable"))?;
    crate::db::rules::insert_user_rule(conn, &item.category_id, kind, &pattern)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::model::NewItem;

    fn seeded_rules() -> Vec<Rule> {
        crate::db::rules::list_active(&Db::open_in_memory().unwrap().conn()).unwrap()
    }

    fn policy() -> Policy {
        Policy { secret_detection: true, ai_enabled: true, local_only: false, model_ready: true, threshold: 0.6 }
    }

    fn text_item(text: &str) -> ItemRow {
        ItemRow {
            id: "t".into(),
            kind: "text".into(),
            title: text.into(),
            payload_text: Some(text.into()),
            source_kind: "drop".into(),
            ..Default::default()
        }
    }

    fn out(category: &str, alternative: &str, confidence: f64) -> TriageOut {
        TriageOut {
            category: category.into(),
            alternative: alternative.into(),
            title: "Fix login bug".into(),
            summary: "Fix the login bug before the Friday release.".into(),
            tags: vec!["bug".into(), "login".into()],
            confidence,
            task: "Send the fix to QA".into(),
        }
    }

    /// Runs `decide` with a model stub that answers `reply` and counts its calls.
    fn run(item: &ItemRow, p: &Policy, reply: AppResult<TriageOut>) -> (Decision, u32) {
        let calls = Cell::new(0);
        let d = tauri::async_runtime::block_on(decide(item, &seeded_rules(), p, || async {
            calls.set(calls.get() + 1);
            reply
        }));
        (d, calls.get())
    }

    #[test]
    fn rule_hit_skips_the_model() {
        let item = ItemRow {
            kind: "url".into(),
            title: "www.figma.com/file/abc".into(),
            url: Some("https://www.figma.com/file/abc".into()),
            source_kind: "drop".into(),
            ..Default::default()
        };
        let (d, calls) = run(&item, &policy(), Ok(out("read", "todo", 0.9)));
        assert_eq!(calls, 0);
        assert_eq!((d.category_id.as_str(), d.source, d.confidence), ("design", "rule", Some(1.0)));
        assert_eq!(d.reason, "rule:domain:figma.com");
        let u = into_update(item, "done", d);
        assert_eq!(u.title.as_deref(), Some("www.figma.com/file/abc"));
        assert_eq!((u.summary, u.tags), (None, None));
    }

    #[test]
    fn confident_answer_is_kept_with_title_summary_and_tags() {
        let item = text_item("Fix the login bug before Friday's release");
        let (d, calls) = run(&item, &policy(), Ok(out("todo", "code", 0.9)));
        assert_eq!(calls, 1);
        assert_eq!((d.category_id.as_str(), d.source, d.reason.as_str()), ("todo", "ai", "ai"));
        assert!(d.suggestions.is_empty());
        let u = into_update(item, "skipped", d);
        assert_eq!(u.title.as_deref(), Some("Fix login bug"));
        assert_eq!(u.summary.as_deref(), Some("Fix the login bug before the Friday release."));
        assert_eq!(u.tags, Some(vec!["bug".to_owned(), "login".to_owned()]));
        assert_eq!(u.triage_confidence, Some(0.9));

        // A file keeps its file-name title.
        let file = ItemRow {
            kind: "file".into(),
            title: "notes.bin".into(),
            file_name: Some("notes.bin".into()),
            source_kind: "drop".into(),
            ..Default::default()
        };
        let (d, _) = run(&file, &policy(), Ok(out("reference", "read", 0.8)));
        assert_eq!(into_update(file, "skipped", d).title.as_deref(), Some("notes.bin"));
    }

    #[test]
    fn unsure_answer_goes_to_unsorted_with_suggestions() {
        let item = text_item("hmm");
        let (d, _) = run(&item, &policy(), Ok(out("todo", "todo", 0.4)));
        assert_eq!((d.category_id.as_str(), d.source, d.confidence), ("unsorted", "ai", Some(0.4)));
        assert_eq!(d.suggestions, vec!["todo"]);

        let (d, _) = run(&item, &policy(), Ok(out("unsorted", "idea", 0.9)));
        assert_eq!(d.category_id, "unsorted");
        assert_eq!(d.suggestions, vec!["idea"]);
        // The summary is still the model's.
        assert!(into_update(item, "skipped", d).summary.is_some());
    }

    #[test]
    fn sensitive_item_never_reaches_the_model() {
        let secret = format!("deploy with sk-ant-api03-{}", "a".repeat(40));
        let item = text_item(&secret);
        let (d, calls) = run(&item, &policy(), Ok(out("code", "todo", 0.9)));
        assert_eq!(calls, 0);
        assert!(d.sensitive);
        assert_eq!((d.category_id.as_str(), d.source, d.reason.as_str()), ("unsorted", "none", "none:sensitive"));

        // With secret detection off the same text is an ordinary item.
        let (d, calls) = run(&item, &Policy { secret_detection: false, ..policy() }, Ok(out("code", "todo", 0.9)));
        assert_eq!((calls, d.sensitive, d.category_id.as_str()), (1, false, "code"));
    }

    #[test]
    fn model_failure_falls_back_to_unsorted() {
        let item = text_item("Fix the login bug before Friday's release");
        let (d, calls) = run(&item, &policy(), Err(AppError::msg("bad json")));
        assert_eq!(calls, 1);
        assert_eq!((d.category_id.as_str(), d.source, d.reason.as_str()), ("unsorted", "none", "none:ai-error"));
        assert_eq!(d.confidence, None);
        assert!(into_update(item, "skipped", d).summary.is_none());
    }

    #[test]
    fn skip_reasons_without_the_model() {
        let item = text_item("Fix the login bug before Friday's release");
        let no_model = Policy { model_ready: false, ..policy() };
        let (d, calls) = run(&item, &no_model, Ok(out("todo", "code", 0.9)));
        assert_eq!((calls, d.reason.as_str()), (0, "none:no-model"));
        let local = Policy { local_only: true, ..no_model };
        let (d, calls) = run(&item, &local, Ok(out("todo", "code", 0.9)));
        assert_eq!((calls, d.reason.as_str()), (0, "none:ai-off"));
    }

    #[test]
    fn make_rule_uses_domain_or_extension() {
        let db = Db::open_in_memory().unwrap();
        let mut conn = db.conn();
        let mut insert = |kind: &str, url: Option<&str>, file_name: Option<&str>, hash: &str| {
            let id = items::insert_item(
                &mut conn,
                &NewItem {
                    kind: kind.into(),
                    title: hash.into(),
                    url: url.map(Into::into),
                    file_name: file_name.map(Into::into),
                    payload_text: (kind == "text").then(|| "hello".into()),
                    source_kind: "drop".into(),
                    content_hash: hash.into(),
                    ..Default::default()
                },
            )
            .unwrap();
            items::recategorize(&mut conn, std::slice::from_ref(&id), "reference", "user").unwrap();
            id
        };
        let url = insert("url", Some("https://www.Example.com/a?b=1"), None, "u");
        let file = insert("file", None, Some("Spec.PDF"), "f");
        let text = insert("text", None, None, "t");

        let r = make_rule_from_item(&mut conn, &url).unwrap();
        assert_eq!((r.kind.as_str(), r.pattern.as_str(), r.category_id.as_str()), ("domain", "example.com", "reference"));
        let r = make_rule_from_item(&mut conn, &file).unwrap();
        assert_eq!((r.kind.as_str(), r.pattern.as_str(), r.position, r.builtin), ("extension", "pdf", 0, false));
        assert!(matches!(make_rule_from_item(&mut conn, &text), Err(AppError::Code("notRuleable"))));
    }
}
