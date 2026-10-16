//! Rule and category-example storage. Matching lives in `triage::rules`.
use rusqlite::{params, Connection};

use crate::db::{new_id, now_ms};
use crate::error::{AppError, AppResult};
use crate::model::{Rule, CATEGORY_IDS};

/// Active rules, user rules first, each group in ascending `position`.
#[allow(dead_code)] // called by the triage worker (Step 9)
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
#[allow(dead_code)] // called by make_rule_from_item (Step 9)
pub fn insert_user_rule(
    conn: &mut Connection,
    category_id: &str,
    kind: &str,
    pattern: &str,
) -> AppResult<Rule> {
    if !CATEGORY_IDS.contains(&category_id) {
        return Err(AppError::msg(format!("unknown category {category_id}")));
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
#[allow(dead_code)] // called by triage AI (Step 9)
pub fn recent_examples(conn: &Connection, limit: usize) -> AppResult<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT e.title, e.category_id FROM category_examples e
         JOIN categories c ON c.id = e.category_id
         WHERE e.deleted_at IS NULL AND c.exclude_from_ai = 0
         ORDER BY e.created_at DESC, e.id DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

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
}
