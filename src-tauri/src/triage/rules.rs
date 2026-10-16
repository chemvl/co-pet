//! Deterministic rules: user rules first (by position), then built-ins (by position); the first
//! match wins. Reasons are language-neutral codes the frontend renders.
use regex::Regex;

use crate::db::search::fold;
use crate::model::{ItemRow, Rule};

#[derive(Debug, Clone, PartialEq)]
pub struct RuleMatch {
    pub rule_id: String,
    pub category_id: String,
    /// `rule:domain:<host>`, `rule:extension:<ext>`, `rule:kind:<kind>`, `rule:source:<kind>`,
    /// `rule:app:<pattern>`, `rule:regex:<category>`.
    pub reason: String,
}

/// URL host, lowercase, without a leading `www.`.
pub(crate) fn host(url: &str) -> Option<String> {
    let host = url::Url::parse(url.trim()).ok()?.host_str()?.to_ascii_lowercase();
    Some(host.strip_prefix("www.").map(str::to_owned).unwrap_or(host))
}

pub(crate) fn extension(file_name: &str) -> Option<String> {
    let (stem, ext) = file_name.rsplit_once('.')?;
    (!stem.is_empty() && !ext.is_empty()).then(|| ext.to_ascii_lowercase())
}

pub(crate) fn matches(rule: &Rule, item: &ItemRow) -> bool {
    match rule.kind.as_str() {
        "domain" => item.url.as_deref().and_then(host).is_some_and(|h| {
            let p = rule.pattern.to_ascii_lowercase();
            let p = p.strip_prefix("www.").unwrap_or(&p);
            h == p || h.ends_with(&format!(".{p}"))
        }),
        // Screenshots have no extension rule: without AI they stay Unsorted.
        "extension" => {
            item.kind != "screenshot"
                && item
                    .file_name
                    .as_deref()
                    .and_then(extension)
                    .is_some_and(|e| e == rule.pattern.to_ascii_lowercase())
        }
        "item_kind" => item.kind == rule.pattern,
        "source_kind" => item.source_kind == rule.pattern,
        "source_app" => {
            if rule.pattern.trim().is_empty() {
                return false;
            }
            let p = fold(rule.pattern.trim());
            item.source_app.as_deref().is_some_and(|a| fold(a.trim()) == p)
                || item.source_bundle_id.as_deref().is_some_and(|b| b.trim().eq_ignore_ascii_case(rule.pattern.trim()))
        }
        "regex" => Regex::new(&rule.pattern).is_ok_and(|re| regex_matches(&re, item)),
        _ => false,
    }
}

/// A `regex` rule's test with its pattern already compiled (callers matching many items).
pub(crate) fn regex_matches(re: &Regex, item: &ItemRow) -> bool {
    item.payload_text.as_deref().is_some_and(|t| re.is_match(t))
}

fn reason(rule: &Rule, item: &ItemRow) -> String {
    match rule.kind.as_str() {
        "domain" => format!("rule:domain:{}", rule.pattern.to_ascii_lowercase()),
        "extension" => format!("rule:extension:{}", rule.pattern.to_ascii_lowercase()),
        "item_kind" => format!("rule:kind:{}", item.kind),
        "source_kind" => format!("rule:source:{}", item.source_kind),
        "source_app" => format!("rule:app:{}", rule.pattern.trim()),
        _ => format!("rule:regex:{}", rule.category_id),
    }
}

pub fn match_rules(item: &ItemRow, rules: &[Rule]) -> Option<RuleMatch> {
    let mut ordered: Vec<&Rule> = rules.iter().collect();
    // User rules (builtin = false) first, each group by ascending position.
    ordered.sort_by_key(|r| (r.builtin, r.position));
    ordered.into_iter().find(|r| matches(r, item)).map(|r| RuleMatch {
        rule_id: r.id.clone(),
        category_id: r.category_id.clone(),
        reason: reason(r, item),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{rules, Db};

    fn builtins() -> Vec<Rule> {
        let db = Db::open_in_memory().unwrap();
        let conn = db.conn();
        rules::list_active(&conn).unwrap()
    }

    fn url_item(url: &str) -> ItemRow {
        ItemRow { kind: "url".into(), source_kind: "drop".into(), url: Some(url.into()), ..Default::default() }
    }

    fn file_item(name: &str) -> ItemRow {
        ItemRow { kind: "file".into(), source_kind: "drop".into(), file_name: Some(name.into()), ..Default::default() }
    }

    #[test]
    fn source_app_rule_matches_name_or_bundle_id() {
        let rule = |pattern: &str| Rule {
            id: "app".into(),
            category_id: "design".into(),
            kind: "source_app".into(),
            pattern: pattern.into(),
            builtin: false,
            position: 0,
        };
        let item = ItemRow {
            kind: "text".into(),
            source_kind: "clipboard".into(),
            source_app: Some("Figma".into()),
            source_bundle_id: Some("com.figma.Desktop".into()),
            ..Default::default()
        };
        let m = match_rules(&item, &[rule("figma")]).unwrap();
        assert_eq!((m.category_id.as_str(), m.reason.as_str()), ("design", "rule:app:figma"));
        assert!(match_rules(&item, &[rule("COM.FIGMA.DESKTOP")]).is_some());
        assert!(match_rules(&item, &[rule("Sketch")]).is_none());
        assert!(match_rules(&ItemRow::default(), &[rule("figma")]).is_none());
    }

    #[test]
    fn user_rule_beats_builtin() {
        let mut all = builtins();
        all.push(Rule {
            id: "user-1".into(),
            category_id: "reference".into(),
            kind: "domain".into(),
            pattern: "figma.com".into(),
            builtin: false,
            position: 0,
        });
        let m = match_rules(&url_item("https://www.figma.com/file/abc"), &all).unwrap();
        assert_eq!(m.rule_id, "user-1");
        assert_eq!(m.category_id, "reference");
    }

    #[test]
    fn domain_matches_host_and_subdomains_only() {
        let all = builtins();
        for u in ["https://www.figma.com/file/abc", "https://design.figma.com/x"] {
            let m = match_rules(&url_item(u), &all).unwrap();
            assert_eq!(m.category_id, "design", "{u}");
            assert_eq!(m.reason, "rule:domain:figma.com");
        }
        let m = match_rules(&url_item("https://notfigma.com/x"), &all).unwrap();
        assert_eq!(m.category_id, "read");
        assert_eq!(m.reason, "rule:kind:url");
    }

    #[test]
    fn quick_note_is_idea() {
        let note = ItemRow {
            kind: "text".into(),
            source_kind: "note".into(),
            payload_text: Some("call mum".into()),
            ..Default::default()
        };
        let m = match_rules(&note, &builtins()).unwrap();
        assert_eq!((m.category_id.as_str(), m.reason.as_str()), ("idea", "rule:source:note"));
    }

    #[test]
    fn ics_is_meeting() {
        let m = match_rules(&file_item("Standup.ICS"), &builtins()).unwrap();
        assert_eq!((m.category_id.as_str(), m.reason.as_str()), ("meeting", "rule:extension:ics"));
    }

    #[test]
    fn url_without_domain_rule_is_read() {
        let m = match_rules(&url_item("https://example.org/article"), &builtins()).unwrap();
        assert_eq!((m.category_id.as_str(), m.reason.as_str()), ("read", "rule:kind:url"));
    }
}
