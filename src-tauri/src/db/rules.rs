//! Rule and category-example storage. Matching lives in `triage::rules`.
use rusqlite::{params, Connection};

use crate::db::{categories, new_id, now_ms};
use crate::error::{AppError, AppResult};
use crate::model::{Example, Rule, RuleView};

/// Active rules, user rules first, each group in ascending `position`.
pub fn list_active(conn: &Connection) -> AppResult<Vec<Rule>> {
    let mut stmt = conn.prepare(
        "SELECT id, category_id, kind, pattern, builtin, position FROM rules
         WHERE deleted_at IS NULL ORDER BY builtin ASC, position ASC",
    )?;
    let rules = stmt
        .query_map([], |r| {
            Ok(Rule {
                id: r.get(0)?,
                category_id: r.get(1)?,
                kind: r.get(2)?,
                pattern: r.get(3)?,
                builtin: r.get(4)?,
                position: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rules)
}

/// Inserts a user rule at position 0 and shifts the other user rules down. An existing user
/// rule with the same kind and pattern is replaced, so the newest choice wins.
pub fn insert_user_rule(
    conn: &mut Connection,
    category_id: &str,
    kind: &str,
    pattern: &str,
) -> AppResult<Rule> {
    if !categories::exists(conn, category_id)? {
        return Err(AppError::Code("notFound"));
    }
    let tx = conn.transaction()?;
    let now = now_ms();
    tx.execute(
        "UPDATE rules SET deleted_at = ?3, updated_at = ?3
         WHERE builtin = 0 AND deleted_at IS NULL AND kind = ?1 AND pattern = ?2",
        params![kind, pattern, now],
    )?;
    tx.execute(
        "UPDATE rules SET position = position + 1, updated_at = ?1
         WHERE builtin = 0 AND deleted_at IS NULL",
        [now],
    )?;
    let rule = Rule {
        id: new_id(),
        category_id: category_id.to_string(),
        kind: kind.to_string(),
        pattern: pattern.to_string(),
        builtin: false,
        position: 0,
    };
    tx.execute(
        "INSERT INTO rules (id, category_id, kind, pattern, builtin, position, updated_at)
         VALUES (?1, ?2, ?3, ?4, 0, 0, ?5)",
        params![rule.id, rule.category_id, rule.kind, rule.pattern, now],
    )?;
    tx.commit()?;
    Ok(rule)
}

/// The newest `(title, category)` correction examples, skipping categories excluded from AI.
pub fn recent_examples(conn: &Connection, limit: usize) -> AppResult<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT e.title, e.category_id FROM category_examples e
         JOIN categories c ON c.id = e.category_id
         JOIN items i ON i.id = e.item_id
         WHERE e.deleted_at IS NULL AND c.deleted_at IS NULL
           AND (c.exclude_from_ai = 0 OR c.id = 'unsorted')
           AND i.sensitive = 0 AND i.deleted_at IS NULL
         ORDER BY e.created_at DESC, e.id DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Live rules of `category_id` (user first, then `position`) with how many of the category's
/// live items each one matches.
pub fn list_for_category(conn: &Connection, category_id: &str) -> AppResult<Vec<RuleView>> {
    let ids: Vec<String> = conn
        .prepare(
            "SELECT id FROM items WHERE category_id = ?1 AND deleted_at IS NULL AND status != 'trashed'",
        )?
        .query_map([category_id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let items = crate::db::items::get_rows(conn, &ids)?;
    Ok(list_active(conn)?
        .into_iter()
        .filter(|r| r.category_id == category_id)
        .map(|rule| {
            let match_count = if rule.kind == "regex" {
                // Compiled once per rule, not once per item.
                match regex::Regex::new(&rule.pattern) {
                    Ok(re) => items.iter().filter(|i| crate::triage::rules::regex_matches(&re, i)).count(),
                    Err(_) => 0,
                }
            } else {
                items.iter().filter(|i| crate::triage::rules::matches(&rule, i)).count()
            } as i64;
            RuleView { rule, match_count }
        })
        .collect())
}

const ITEM_KINDS: [&str; 7] = ["file", "image", "url", "text", "richtext", "code", "screenshot"];
const SOURCE_KINDS: [&str; 5] = ["drop", "clipboard", "screenshot", "note", "selection"];

/// The canonical pattern for a user-entered rule, or `invalid`.
fn normalize(kind: &str, pattern: &str) -> AppResult<String> {
    let p = pattern.trim();
    let invalid = || AppError::Code("invalid");
    match kind {
        "domain" => {
            let lower = p.to_ascii_lowercase();
            let with_scheme =
                if lower.contains("://") { lower } else { format!("http://{lower}") };
            let host = crate::triage::rules::host(&with_scheme).ok_or_else(invalid)?;
            (host.contains('.') && !host.starts_with('.') && !host.ends_with('.'))
                .then_some(host)
                .ok_or_else(invalid)
        }
        "extension" => {
            let ext = p.strip_prefix('.').unwrap_or(p).to_ascii_lowercase();
            (!ext.is_empty()
                && ext.len() <= 10
                && ext.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
            .then_some(ext)
            .ok_or_else(invalid)
        }
        "item_kind" => ITEM_KINDS.contains(&p).then(|| p.to_string()).ok_or_else(invalid),
        "source_kind" => SOURCE_KINDS.contains(&p).then(|| p.to_string()).ok_or_else(invalid),
        "source_app" => (!p.is_empty() && p.chars().count() <= 100)
            .then(|| p.to_string())
            .ok_or_else(invalid),
        "regex" => (!p.is_empty() && pattern.chars().count() <= 500 && regex::Regex::new(pattern).is_ok())
            .then(|| pattern.to_string())
            .ok_or_else(invalid),
        _ => Err(invalid()),
    }
}

/// Validates and normalizes a user-entered rule, then inserts it at the top of the user rules.
pub fn add_rule(conn: &mut Connection, category_id: &str, kind: &str, pattern: &str) -> AppResult<Rule> {
    let pattern = normalize(kind, pattern)?;
    insert_user_rule(conn, category_id, kind, &pattern)
}

fn set_deleted(conn: &Connection, table: &str, id: &str, deleted: bool) -> AppResult<()> {
    let now = now_ms();
    let sql = format!(
        "UPDATE {table} SET deleted_at = ?2, updated_at = ?3 WHERE id = ?1 AND deleted_at IS {}",
        if deleted { "NULL" } else { "NOT NULL" }
    );
    let changed = conn.execute(&sql, params![id, deleted.then_some(now), now])?;
    if changed == 0 {
        return Err(AppError::Code("notFound"));
    }
    Ok(())
}

pub fn delete_rule(conn: &Connection, id: &str) -> AppResult<()> {
    set_deleted(conn, "rules", id, true)
}

/// Undo for `delete_rule`.
pub fn restore_rule(conn: &Connection, id: &str) -> AppResult<()> {
    set_deleted(conn, "rules", id, false)
}

/// Live correction examples of `category_id`, newest first.
pub fn list_examples(conn: &Connection, category_id: &str) -> AppResult<Vec<Example>> {
    let mut stmt = conn.prepare(
        "SELECT id, item_id, title, created_at FROM category_examples
         WHERE category_id = ?1 AND deleted_at IS NULL ORDER BY created_at DESC, id DESC",
    )?;
    let rows = stmt
        .query_map([category_id], |r| {
            Ok(Example { id: r.get(0)?, item_id: r.get(1)?, title: r.get(2)?, created_at: r.get(3)? })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn delete_example(conn: &Connection, id: &str) -> AppResult<()> {
    set_deleted(conn, "category_examples", id, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn item(conn: &mut Connection, hash: &str, url: &str) -> String {
        let new = crate::model::NewItem {
            kind: "url".into(),
            title: hash.into(),
            url: Some(url.into()),
            source_kind: "drop".into(),
            content_hash: hash.into(),
            ..Default::default()
        };
        crate::db::items::insert_item(conn, &new).unwrap()
    }

    #[test]
    fn add_rule_normalizes_domain() {
        let db = Db::open_in_memory().unwrap();
        let mut conn = db.conn();
        let r = add_rule(&mut conn, "read", "domain", "https://www.Figma.com/file/x").unwrap();
        assert_eq!(r.pattern, "figma.com");
        assert!(matches!(add_rule(&mut conn, "read", "domain", "localhost:8080"), Err(AppError::Code("invalid"))));
        assert_eq!(add_rule(&mut conn, "read", "extension", ".PDF").unwrap().pattern, "pdf");
        assert_eq!(add_rule(&mut conn, "read", "source_app", "  Figma ").unwrap().pattern, "Figma");
        assert!(add_rule(&mut conn, "read", "source_app", "   ").is_err());
        assert!(add_rule(&mut conn, "read", "item_kind", "video").is_err());
        assert!(add_rule(&mut conn, "read", "bogus", "x").is_err());
    }

    #[test]
    fn add_rule_rejects_bad_regex() {
        let db = Db::open_in_memory().unwrap();
        let mut conn = db.conn();
        assert!(matches!(add_rule(&mut conn, "read", "regex", "(unclosed"), Err(AppError::Code("invalid"))));
        assert!(matches!(add_rule(&mut conn, "read", "regex", &"a".repeat(501)), Err(AppError::Code("invalid"))));
        assert!(matches!(add_rule(&mut conn, "read", "regex", ""), Err(AppError::Code("invalid"))));
        assert!(matches!(add_rule(&mut conn, "read", "regex", "  \t "), Err(AppError::Code("invalid"))));
        assert!(add_rule(&mut conn, "read", "regex", "^todo").is_ok());
    }

    #[test]
    fn regex_match_count_uses_payload_text() {
        let db = Db::open_in_memory().unwrap();
        let mut conn = db.conn();
        let rule = add_rule(&mut conn, "read", "regex", "^invoice").unwrap();
        let mut ids = Vec::new();
        for (hash, text) in [("a", "invoice 42"), ("b", "Invoice later"), ("c", "an invoice")] {
            let new = crate::model::NewItem {
                kind: "text".into(),
                title: hash.into(),
                payload_text: Some(text.into()),
                source_kind: "clipboard".into(),
                content_hash: hash.into(),
                ..Default::default()
            };
            ids.push(crate::db::items::insert_item(&mut conn, &new).unwrap());
        }
        crate::db::items::recategorize(&mut conn, &ids, "read", &crate::model::Actor::User).unwrap();
        let views = list_for_category(&conn, "read").unwrap();
        assert_eq!(views.iter().find(|v| v.rule.id == rule.id).unwrap().match_count, 1);
    }

    #[test]
    fn match_count_counts_only_category_items() {
        let db = Db::open_in_memory().unwrap();
        let mut conn = db.conn();
        let rule = add_rule(&mut conn, "read", "domain", "figma.com").unwrap();
        let a = item(&mut conn, "a", "https://figma.com/a");
        let b = item(&mut conn, "b", "https://www.figma.com/b");
        let c = item(&mut conn, "c", "https://example.com/c");
        let d = item(&mut conn, "d", "https://figma.com/d");
        crate::db::items::recategorize(&mut conn, &[a, b, c], "read", &crate::model::Actor::User).unwrap();
        crate::db::items::recategorize(&mut conn, &[d], "code", &crate::model::Actor::User).unwrap();
        let views = list_for_category(&conn, "read").unwrap();
        let view = views.iter().find(|v| v.rule.id == rule.id).unwrap();
        assert_eq!(view.match_count, 2);
        assert!(views.iter().all(|v| v.rule.category_id == "read"));
    }

    #[test]
    fn delete_then_restore_rule() {
        let db = Db::open_in_memory().unwrap();
        let mut conn = db.conn();
        let rule = add_rule(&mut conn, "read", "domain", "figma.com").unwrap();
        delete_rule(&conn, &rule.id).unwrap();
        assert!(!list_active(&conn).unwrap().iter().any(|r| r.id == rule.id));
        assert!(matches!(delete_rule(&conn, &rule.id), Err(AppError::Code("notFound"))));
        restore_rule(&conn, &rule.id).unwrap();
        assert!(list_active(&conn).unwrap().iter().any(|r| r.id == rule.id));
    }

    #[test]
    fn examples_list_and_delete() {
        let db = Db::open_in_memory().unwrap();
        let mut conn = db.conn();
        let a = item(&mut conn, "a", "https://figma.com/a");
        crate::db::items::recategorize(&mut conn, &[a.clone()], "read", &crate::model::Actor::User).unwrap();
        let ex = list_examples(&conn, "read").unwrap();
        assert_eq!(ex.len(), 1);
        assert_eq!(ex[0].item_id, a);
        delete_example(&conn, &ex[0].id).unwrap();
        assert!(list_examples(&conn, "read").unwrap().is_empty());
    }

    #[test]
    fn user_rules_come_first_newest_at_top() {
        let db = Db::open_in_memory().unwrap();
        let mut conn = db.conn();
        let a = insert_user_rule(&mut conn, "read", "domain", "a.com").unwrap();
        let b = insert_user_rule(&mut conn, "code", "domain", "b.com").unwrap();
        let rules = list_active(&conn).unwrap();
        assert_eq!(
            (rules[0].id.as_str(), rules[0].position),
            (b.id.as_str(), 0)
        );
        assert_eq!(
            (rules[1].id.as_str(), rules[1].position),
            (a.id.as_str(), 1)
        );
        assert!(rules[2].builtin);
        assert_eq!(rules[2].pattern, "note");
    }

    #[test]
    fn examples_only_name_categories_the_model_may_answer() {
        let db = Db::open_in_memory().unwrap();
        let mut conn = db.conn();
        for (i, cat) in ["read", "meeting", "code", "unsorted"].into_iter().enumerate() {
            let new = crate::model::NewItem {
                kind: "text".into(),
                title: cat.into(),
                payload_text: Some(cat.into()),
                source_kind: "drop".into(),
                content_hash: format!("h{i}"),
                ..Default::default()
            };
            let id = crate::db::items::insert_item(&mut conn, &new).unwrap();
            crate::db::items::recategorize(&mut conn, &[id], cat, &crate::model::Actor::User).unwrap();
        }
        conn.execute_batch(
            "UPDATE categories SET deleted_at = 1 WHERE id = 'meeting';
             UPDATE categories SET exclude_from_ai = 1 WHERE id IN ('code', 'unsorted');",
        )
        .unwrap();
        let mut cats: Vec<String> = recent_examples(&conn, 8).unwrap().into_iter().map(|(_, c)| c).collect();
        cats.sort();
        assert_eq!(cats, ["read", "unsorted"]);
    }
}
