//! Category rows: the nine built-ins plus the user's own. Deletes are soft; `unsorted` can't be
//! deleted and always sorts last.
use rusqlite::{params, Connection, OptionalExtension, Row};
use tauri::AppHandle;

use crate::db::{items, new_id, now_ms, search};
use crate::error::{AppError, AppResult};
use crate::events;
use crate::i18n::{tr, Lang};
use crate::model::{Actor, Category, CategoryPatch, BUILTIN_CATEGORY_IDS};

/// The ds `IconName`s (`src/ds/icons.ts`); a custom category's icon is one of these.
const ICONS: &[&str] = &[
    "whats-next", "screenshot", "clipboard", "mic", "home", "note", "todo", "read", "reference",
    "code", "design", "idea", "contact", "meeting", "unsorted", "inbox", "tasks", "search", "copy",
    "paste", "pin", "more", "close", "check", "plus", "chevron-down", "play", "pause", "stop",
    "snooze", "skip", "link", "image", "file", "text", "lock", "agent", "sync", "export", "alert",
    "settings", "calendar", "clock", "language", "grip", "trash", "undo", "moon", "sun", "monitor",
];

const NAME_MAX: usize = 40;
const UNSORTED: &str = "unsorted";
const COLS: &str = "id, name, icon, color, position, exclude_from_ai, exclude_from_mcp";

fn map_row(r: &Row<'_>) -> rusqlite::Result<Category> {
    let id: String = r.get(0)?;
    Ok(Category {
        builtin: BUILTIN_CATEGORY_IDS.contains(&id.as_str()),
        id,
        name: r.get(1)?,
        icon: r.get(2)?,
        color: r.get(3)?,
        position: r.get(4)?,
        exclude_from_ai: r.get(5)?,
        exclude_from_mcp: r.get(6)?,
    })
}

/// Live rows, in display order.
pub fn list(conn: &Connection) -> AppResult<Vec<Category>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM categories WHERE deleted_at IS NULL ORDER BY position, id"
    ))?;
    let rows = stmt.query_map([], map_row)?.collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Whether `id` is a live category.
pub fn exists(conn: &Connection, id: &str) -> AppResult<bool> {
    let found = conn
        .query_row(
            "SELECT 1 FROM categories WHERE id = ?1 AND deleted_at IS NULL",
            [id],
            |_| Ok(()),
        )
        .optional()?;
    Ok(found.is_some())
}

fn get(conn: &Connection, id: &str) -> AppResult<Category> {
    conn.query_row(
        &format!("SELECT {COLS} FROM categories WHERE id = ?1 AND deleted_at IS NULL"),
        [id],
        map_row,
    )
    .optional()?
    .ok_or(AppError::Code("notFound"))
}

/// A built-in id's name in every UI language.
fn builtin_names(id: &str) -> [&'static str; Lang::ALL.len()] {
    let key = format!("category.{id}");
    Lang::ALL.map(|l| tr(l, &key))
}

/// The name a live category shows in `lang`: its own, else the built-in name.
pub fn display_name(conn: &Connection, id: &str, lang: Lang) -> AppResult<String> {
    let c = get(conn, id)?;
    Ok(c.name.unwrap_or_else(|| tr(lang, &format!("category.{}", c.id)).to_string()))
}

/// The names a category shows: its own, or every language's built-in name.
fn display_names(c: &Category) -> Vec<&str> {
    match &c.name {
        Some(name) => vec![name.as_str()],
        None if c.builtin => builtin_names(&c.id).to_vec(),
        None => Vec::new(),
    }
}

/// Trimmed name of 1–40 characters, else `invalid`.
fn clean_name(name: &str) -> AppResult<&str> {
    let name = name.trim();
    let len = name.chars().count();
    if (1..=NAME_MAX).contains(&len) {
        Ok(name)
    } else {
        Err(AppError::Code("invalid"))
    }
}

fn check_icon(icon: &str) -> AppResult<()> {
    if ICONS.contains(&icon) {
        Ok(())
    } else {
        Err(AppError::Code("invalid"))
    }
}

/// A colour is any built-in id except `unsorted`.
fn check_color(color: &str) -> AppResult<()> {
    if color != UNSORTED && BUILTIN_CATEGORY_IDS.contains(&color) {
        Ok(())
    } else {
        Err(AppError::Code("invalid"))
    }
}

/// `duplicate` when another live category (not `except`) shows `name`, compared folded.
fn check_unique(conn: &Connection, name: &str, except: Option<&str>) -> AppResult<()> {
    let folded = search::fold(name);
    for c in list(conn)? {
        if Some(c.id.as_str()) == except {
            continue;
        }
        if display_names(&c).iter().any(|n| search::fold(n) == folded) {
            return Err(AppError::Code("duplicate"));
        }
    }
    Ok(())
}

/// Adds a category before `unsorted`.
pub fn create(conn: &mut Connection, name: &str, icon: &str, color: &str) -> AppResult<Category> {
    let name = clean_name(name)?;
    check_icon(icon)?;
    check_color(color)?;
    let tx = conn.transaction()?;
    check_unique(&tx, name, None)?;
    let id = new_id();
    let now = now_ms();
    let max: i64 = tx.query_row(
        "SELECT COALESCE(MAX(position), -1) FROM categories WHERE deleted_at IS NULL",
        [],
        |r| r.get(0),
    )?;
    tx.execute(
        "INSERT INTO categories (id, name, icon, color, position, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, name, icon, color, max + 1, now],
    )?;
    tx.execute(
        "UPDATE categories SET position = ?2, updated_at = ?3 WHERE id = ?1",
        params![UNSORTED, max + 2, now],
    )?;
    let category = get(&tx, &id)?;
    tx.commit()?;
    Ok(category)
}

/// Applies `patch`. `unsorted` may only be renamed (`notEditable`); a built-in renamed to its
/// own English or Vietnamese name goes back to `NULL` (follows the UI language).
pub fn update(conn: &mut Connection, id: &str, patch: &CategoryPatch) -> AppResult<Category> {
    let tx = conn.transaction()?;
    let mut c = get(&tx, id)?;
    if id == UNSORTED
        && (patch.icon.is_some()
            || patch.color.is_some()
            || patch.exclude_from_ai.is_some()
            || patch.exclude_from_mcp.is_some())
    {
        return Err(AppError::Code("notEditable"));
    }
    if let Some(name) = &patch.name {
        let name = clean_name(name)?;
        check_unique(&tx, name, Some(id))?;
        let builtin_name = c.builtin && builtin_names(id).contains(&name);
        c.name = (!builtin_name).then(|| name.to_owned());
    }
    if let Some(icon) = &patch.icon {
        check_icon(icon)?;
        c.icon = Some(icon.clone());
    }
    if let Some(color) = &patch.color {
        check_color(color)?;
        c.color = Some(color.clone());
    }
    if let Some(v) = patch.exclude_from_ai {
        c.exclude_from_ai = v;
    }
    if let Some(v) = patch.exclude_from_mcp {
        c.exclude_from_mcp = v;
    }
    tx.execute(
        "UPDATE categories SET name = ?2, icon = ?3, color = ?4, exclude_from_ai = ?5,
           exclude_from_mcp = ?6, updated_at = ?7
         WHERE id = ?1",
        params![
            id,
            c.name,
            c.icon,
            c.color,
            c.exclude_from_ai,
            c.exclude_from_mcp,
            now_ms()
        ],
    )?;
    tx.commit()?;
    Ok(c)
}

/// Soft-deletes the category and its rules after moving its live items to `unsorted` (a user
/// move that teaches the model nothing). Returns the moved item ids and their move events (for
/// Undo per item).
pub fn delete(conn: &mut Connection, id: &str) -> AppResult<(Vec<String>, Vec<String>)> {
    if id == UNSORTED {
        return Err(AppError::Code("notDeletable"));
    }
    let tx = conn.transaction()?;
    if !exists(&tx, id)? {
        return Err(AppError::Code("notFound"));
    }
    let ids = tx
        .prepare(
            "SELECT id FROM items WHERE category_id = ?1 AND deleted_at IS NULL ORDER BY captured_at",
        )?
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    let events = items::recategorize_in(&tx, &ids, UNSORTED, &Actor::User, false)?;
    let now = now_ms();
    tx.execute(
        "UPDATE rules SET deleted_at = ?2, updated_at = ?2 WHERE category_id = ?1 AND deleted_at IS NULL",
        params![id, now],
    )?;
    tx.execute(
        "UPDATE categories SET deleted_at = ?2, updated_at = ?2 WHERE id = ?1",
        params![id, now],
    )?;
    tx.commit()?;
    Ok((ids, events))
}

/// Rewrites positions 0..n in the order of `ids`, which must be every live id once; `unsorted`
/// is put last wherever it appears.
pub fn reorder(conn: &mut Connection, ids: &[String]) -> AppResult<()> {
    let tx = conn.transaction()?;
    let mut live: Vec<String> = list(&tx)?.into_iter().map(|c| c.id).collect();
    let mut given = ids.to_vec();
    live.sort_unstable();
    given.sort_unstable();
    if live != given {
        return Err(AppError::Code("invalid"));
    }
    let now = now_ms();
    let ordered = ids
        .iter()
        .map(String::as_str)
        .filter(|id| *id != UNSORTED)
        .chain([UNSORTED]);
    for (position, id) in ordered.enumerate() {
        tx.execute(
            "UPDATE categories SET position = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, position as i64, now],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// After a write to categories, rules or examples: emits `categories-changed` and wakes sync.
pub fn notify_changed(app: &AppHandle) {
    events::categories_changed(app);
    crate::sync::poke(app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::items::{get_row, insert_item};
    use crate::db::rules::insert_user_rule;
    use crate::db::Db;
    use crate::model::NewItem;

    fn db() -> Db {
        Db::open_in_memory().unwrap()
    }

    fn ids(conn: &Connection) -> Vec<String> {
        list(conn).unwrap().into_iter().map(|c| c.id).collect()
    }

    fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn patch_name(name: &str) -> CategoryPatch {
        CategoryPatch { name: Some(name.into()), ..Default::default() }
    }

    #[test]
    fn list_returns_builtins_in_order() {
        let db = db();
        let conn = db.conn();
        let cats = list(&conn).unwrap();
        assert_eq!(ids(&conn), BUILTIN_CATEGORY_IDS.to_vec());
        assert!(cats.iter().all(|c| c.builtin && c.name.is_none()));
        assert!(exists(&conn, "design").unwrap());
        assert!(!exists(&conn, "nope").unwrap());
    }

    #[test]
    fn delete_moves_items_to_unsorted_and_drops_rules() {
        let db = db();
        let mut conn = db.conn();
        let item = insert_item(
            &mut conn,
            &NewItem {
                kind: "text".into(),
                title: "Moodboard".into(),
                payload_text: Some("colours".into()),
                source_kind: "drop".into(),
                content_hash: "h".into(),
                ..Default::default()
            },
        )
        .unwrap();
        conn.execute("UPDATE items SET category_id = 'design' WHERE id = ?1", [&item])
            .unwrap();
        let rule = insert_user_rule(&mut conn, "design", "domain", "dribbble.com").unwrap();
        let examples = count(&conn, "SELECT COUNT(*) FROM category_examples");

        let (moved, events) = delete(&mut conn, "design").unwrap();

        assert_eq!(moved, vec![item.clone()]);
        assert_eq!(events.len(), 1);
        assert_eq!(get_row(&conn, &item).unwrap().category_id, "unsorted");
        let (actor, action, item_id): (String, String, String) = conn
            .query_row(
                "SELECT actor, action, item_id FROM item_events WHERE id = ?1",
                [&events[0]],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((actor.as_str(), action.as_str(), item_id.as_str()), ("user", "recategorize", item.as_str()));
        assert_eq!(
            count(&conn, "SELECT COUNT(*) FROM item_events WHERE action = 'recategorize'"),
            1
        );
        let rule_deleted: Option<i64> = conn
            .query_row("SELECT deleted_at FROM rules WHERE id = ?1", [&rule.id], |r| r.get(0))
            .unwrap();
        assert!(rule_deleted.is_some());
        assert_eq!(
            count(&conn, "SELECT COUNT(*) FROM rules WHERE category_id = 'design' AND deleted_at IS NULL"),
            0
        );
        // Moving items out of a deleted category teaches the model nothing.
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM category_examples"), examples);
        assert!(!exists(&conn, "design").unwrap());
        assert!(!ids(&conn).contains(&"design".to_string()));
    }

    #[test]
    fn unsorted_cannot_be_deleted() {
        let db = db();
        let mut conn = db.conn();
        assert!(matches!(delete(&mut conn, "unsorted"), Err(AppError::Code("notDeletable"))));
        assert!(exists(&conn, "unsorted").unwrap());
    }

    #[test]
    fn create_rejects_bad_color() {
        let db = db();
        let mut conn = db.conn();
        for color in ["unsorted", "pink"] {
            assert!(matches!(
                create(&mut conn, "Recipes", "note", color),
                Err(AppError::Code("invalid"))
            ));
        }
        assert!(matches!(create(&mut conn, "Recipes", "nope", "idea"), Err(AppError::Code("invalid"))));
        assert!(matches!(create(&mut conn, "   ", "note", "idea"), Err(AppError::Code("invalid"))));
        assert!(matches!(
            create(&mut conn, &"x".repeat(41), "note", "idea"),
            Err(AppError::Code("invalid"))
        ));
        assert_eq!(ids(&conn).len(), 9);
    }

    #[test]
    fn create_rejects_duplicate_name() {
        let db = db();
        let mut conn = db.conn();
        for name in ["Design", "thiết kế"] {
            assert!(matches!(create(&mut conn, name, "note", "idea"), Err(AppError::Code("duplicate"))));
        }
        let c = create(&mut conn, "  Recipes ", "note", "idea").unwrap();
        assert_eq!(c.name.as_deref(), Some("Recipes"));
        assert!(!c.builtin);
        assert!(matches!(create(&mut conn, "recipes", "note", "idea"), Err(AppError::Code("duplicate"))));
    }

    #[test]
    fn create_inserts_before_unsorted() {
        let db = db();
        let mut conn = db.conn();
        let c = create(&mut conn, "Recipes", "note", "idea").unwrap();
        let order = ids(&conn);
        assert_eq!(order.len(), 10);
        assert_eq!(order[8], c.id);
        assert_eq!(order[9], "unsorted");
    }

    #[test]
    fn reorder_keeps_unsorted_last() {
        let db = db();
        let mut conn = db.conn();
        let mut order: Vec<String> = ids(&conn);
        order.reverse(); // unsorted first
        reorder(&mut conn, &order).unwrap();
        let got = ids(&conn);
        assert_eq!(got[0], "meeting");
        assert_eq!(got[7], "todo");
        assert_eq!(got[8], "unsorted");
        let positions: Vec<i64> = list(&conn).unwrap().iter().map(|c| c.position).collect();
        assert_eq!(positions, (0..9).collect::<Vec<i64>>());

        // Not a permutation of the live ids.
        assert!(matches!(reorder(&mut conn, &order[1..]), Err(AppError::Code("invalid"))));
        let mut dup = order.clone();
        dup[0] = dup[1].clone();
        assert!(matches!(reorder(&mut conn, &dup), Err(AppError::Code("invalid"))));
    }

    #[test]
    fn rename_to_builtin_name_stores_null() {
        let db = db();
        let mut conn = db.conn();
        let c = update(&mut conn, "design", &patch_name("Moodboards")).unwrap();
        assert_eq!(c.name.as_deref(), Some("Moodboards"));
        for name in ["Design", "Thiết kế"] {
            update(&mut conn, "design", &patch_name("Moodboards")).unwrap();
            let c = update(&mut conn, "design", &patch_name(name)).unwrap();
            assert_eq!(c.name, None);
            assert_eq!(get(&conn, "design").unwrap().name, None);
        }
    }

    #[test]
    fn unsorted_may_only_be_renamed() {
        let db = db();
        let mut conn = db.conn();
        let hide = CategoryPatch { exclude_from_ai: Some(true), ..Default::default() };
        assert!(matches!(update(&mut conn, "unsorted", &hide), Err(AppError::Code("notEditable"))));
        let c = update(&mut conn, "unsorted", &patch_name("Later")).unwrap();
        assert_eq!(c.name.as_deref(), Some("Later"));
        let c = update(&mut conn, "code", &hide).unwrap();
        assert!(c.exclude_from_ai);
        assert!(matches!(update(&mut conn, "code", &patch_name("Design")), Err(AppError::Code("duplicate"))));
    }
}
