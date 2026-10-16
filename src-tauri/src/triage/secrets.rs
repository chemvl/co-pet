//! Secret detection. This curated list is the entire detector: a match marks the item
//! `sensitive`, which keeps it away from the model, previews and the search body.
use std::sync::LazyLock;

use regex::Regex;

use crate::model::ItemRow;

static PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        // AWS access key id
        r"\b(AKIA|ASIA)[0-9A-Z]{16}\b",
        // GitHub tokens
        r"\bgh[pousr]_[A-Za-z0-9]{36,}\b",
        r"\bgithub_pat_[A-Za-z0-9_]{60,}\b",
        // Slack
        r"\bxox[abprs]-[A-Za-z0-9-]{10,}\b",
        // OpenAI
        r"\bsk-(proj-)?[A-Za-z0-9_-]{20,}\b",
        // Anthropic
        r"\bsk-ant-[A-Za-z0-9_-]{20,}\b",
        // Google API
        r"\bAIza[0-9A-Za-z_-]{35}\b",
        // Stripe
        r"\b(sk|rk)_live_[0-9A-Za-z]{20,}\b",
        // Private key block
        r"-----BEGIN [A-Z ]*PRIVATE KEY-----",
        // JWT
        r"\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b",
        // Password-like assignment
        r"(?i)\b(password|passwd|pwd|secret|api[_-]?key|token)\s*[:=]\s*\S{6,}",
    ]
    .iter()
    .map(|p| Regex::new(p).expect("valid secret pattern"))
    .collect()
});

static CARD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:\d[ -]?){13,19}\b").expect("valid card pattern"));

fn luhn_valid(digits: &[u32]) -> bool {
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| match i % 2 {
            0 => d,
            _ if d * 2 > 9 => d * 2 - 9,
            _ => d * 2,
        })
        .sum();
    sum.is_multiple_of(10)
}

pub fn is_sensitive(text: &str) -> bool {
    PATTERNS.iter().any(|re| re.is_match(text))
        || CARD.find_iter(text).any(|m| {
            let digits: Vec<u32> = m.as_str().chars().filter_map(|c| c.to_digit(10)).collect();
            (13..=19).contains(&digits.len()) && luhn_valid(&digits)
        })
}

/// Applies [`is_sensitive`] to everything a secret can hide in: payload text, extracted text,
/// the URL query string and the file name. The caller skips this when
/// `privacy.secretDetection` is off.
#[allow(dead_code)] // called by the triage worker
pub fn is_sensitive_item(item: &ItemRow) -> bool {
    let query = item
        .url
        .as_deref()
        .and_then(|u| url::Url::parse(u).ok())
        .and_then(|u| u.query().map(str::to_owned));
    let fields = [item.payload_text.as_deref(), item.extracted_text.as_deref(), query.as_deref(), item.file_name.as_deref()];
    fields.iter().any(|f| f.is_some_and(is_sensitive))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_pattern_positive() {
        let samples = [
            "key AKIAIOSFODNN7EXAMPLE here",
            "ASIAY34FZKBOKMUTVV7A",
            "ghp_0123456789abcdefghijklmnopqrstuvwxyzAB",
            "github_pat_11ABCDEFG0123456789_abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123",
            "xoxb-1234567890-abcdefghij",
            "sk-abcdefghijklmnopqrstuvwxyz012345",
            "sk-proj-abcdefghijklmnopqrstuvwxyz012345",
            "sk-ant-api03-abcdefghijklmnopqrstuvwxyz",
            "AIzaSyA1234567890abcdefghijklmnopqrstuv",
            "sk_live_0123456789abcdefghijABCD",
            "rk_live_0123456789abcdefghijABCD",
            "-----BEGIN RSA PRIVATE KEY-----\nMIIE...",
            "-----BEGIN PRIVATE KEY-----",
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
            "password: hunter22",
            "API_KEY=abcdef123456",
            "Token = qwerty12345",
            "card 4111 1111 1111 1111 exp 12/29",
            "4242-4242-4242-4242",
            "5555555555554444",
        ];
        for s in samples {
            assert!(is_sensitive(s), "expected sensitive: {s}");
        }
    }

    #[test]
    fn luhn_failing_number_is_negative() {
        assert!(!is_sensitive("order 4111111111111112 shipped"));
        assert!(!is_sensitive("4111 1111 1111 1112"));
    }

    #[test]
    fn sk_inside_a_word_is_negative() {
        assert!(!is_sensitive("task-list-for-the-quarter-planning-2026"));
        assert!(!is_sensitive("Fix the login bug before Friday's release"));
    }

    #[test]
    fn item_query_and_file_name_are_checked() {
        let url_item = ItemRow {
            url: Some("https://example.com/cb?token=abcdef123456&x=1".into()),
            ..Default::default()
        };
        assert!(is_sensitive_item(&url_item));
        let plain_url = ItemRow { url: Some("https://example.com/docs/token".into()), ..Default::default() };
        assert!(!is_sensitive_item(&plain_url));
        let file_item = ItemRow { file_name: Some("AKIAIOSFODNN7EXAMPLE.txt".into()), ..Default::default() };
        assert!(is_sensitive_item(&file_item));
    }
}
