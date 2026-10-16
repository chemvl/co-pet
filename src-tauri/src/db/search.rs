//! Diacritic folding and FTS5 query building.
//!
//! FTS5's `remove_diacritics 2` does not fold `đ`/`Đ`, so both the indexed body and the query are
//! folded here first: NFD, drop `Mn` marks, `đ`/`Đ` → `d`, lowercase.
use std::sync::LazyLock;

use regex::Regex;
use rusqlite::{params, Connection};
use unicode_normalization::UnicodeNormalization;

use crate::error::AppResult;
use crate::model::ItemRow;

static NONSPACING_MARK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\p{Mn}+").unwrap());

pub fn fold(s: &str) -> String {
    let decomposed: String = s.nfd().collect();
    NONSPACING_MARK
        .replace_all(&decomposed, "")
        .chars()
        .map(|c| if c == 'đ' || c == 'Đ' { 'd' } else { c })
        .flat_map(char::to_lowercase)
        .collect()
}

/// Replaces the item's `items_fts` row. A sensitive item indexes its title only, so it can be
/// found by title but never by its content.
pub fn index_item(conn: &Connection, item: &ItemRow) -> AppResult<()> {
    let body = if item.sensitive {
        fold(&item.title)
    } else {
        let parts = [
            item.title.as_str(),
            item.summary.as_deref().unwrap_or(""),
            &item.tags.join(" "),
            item.payload_text.as_deref().unwrap_or(""),
            item.extracted_text.as_deref().unwrap_or(""),
        ];
        fold(&parts.join("\n"))
    };
    unindex(conn, &item.id)?;
    conn.execute(
        "INSERT INTO items_fts (item_id, body) VALUES (?1, ?2)",
        params![item.id, body],
    )?;
    Ok(())
}

/// Removes the item's `items_fts` row.
pub fn unindex(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute("DELETE FROM items_fts WHERE item_id = ?1", [id])?;
    Ok(())
}

/// FTS5 MATCH expression for a user query: folded whitespace tokens as `"tok"*` prefix terms
/// (implicit AND). `None` when the query has no tokens, meaning no FTS filter.
pub fn search_query(q: &str) -> Option<String> {
    let terms: Vec<String> = fold(q)
        .split_whitespace()
        .map(|t| t.replace('"', ""))
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_strips_vietnamese_marks_and_d_stroke() {
        assert_eq!(fold("Đường đến Việt Nam"), "duong den viet nam");
    }

    #[test]
    fn query_builds_prefix_terms_and_drops_quotes() {
        assert_eq!(
            search_query("  Việt \"Nam\" ").as_deref(),
            Some("\"viet\"* \"nam\"*")
        );
        assert_eq!(search_query(" \" "), None);
    }
}
