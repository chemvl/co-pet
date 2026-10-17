//! Word and character error rates for the speech-to-text benchmark. Text is lowercased and
//! punctuation dropped; diacritics are kept, so "hôm" vs "hom" is an error.
use std::sync::LazyLock;

use regex::Regex;

static PUNCT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\p{P}").expect("valid regex"));

fn normalize(text: &str) -> String {
    PUNCT.replace_all(&text.to_lowercase(), " ").into_owned()
}

/// Levenshtein distance between `reference` and `hypothesis` over `reference.len()` (0 when both
/// are empty, 1 for a non-empty hypothesis against an empty reference).
fn rate<T: PartialEq>(reference: &[T], hypothesis: &[T]) -> f64 {
    if reference.is_empty() {
        return if hypothesis.is_empty() { 0.0 } else { 1.0 };
    }
    let mut row: Vec<usize> = (0..=hypothesis.len()).collect();
    for (i, r) in reference.iter().enumerate() {
        let mut diag = row[0];
        row[0] = i + 1;
        for (j, h) in hypothesis.iter().enumerate() {
            let next = (diag + usize::from(r != h)).min(row[j] + 1).min(row[j + 1] + 1);
            diag = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[hypothesis.len()] as f64 / reference.len() as f64
}

/// Word error rate: word-level edit distance over the reference's word count.
pub fn wer(reference: &str, hypothesis: &str) -> f64 {
    let (r, h) = (normalize(reference), normalize(hypothesis));
    let r: Vec<&str> = r.split_whitespace().collect();
    let h: Vec<&str> = h.split_whitespace().collect();
    rate(&r, &h)
}

/// Character error rate, over characters other than whitespace and punctuation.
pub fn cer(reference: &str, hypothesis: &str) -> f64 {
    let chars = |s: &str| -> Vec<char> { normalize(s).chars().filter(|c| !c.is_whitespace()).collect() };
    rate(&chars(reference), &chars(hypothesis))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_is_zero() {
        assert_eq!(wer("Chào mọi người.", "chào, mọi người"), 0.0);
        assert_eq!(cer("Hello world!", "hello world"), 0.0);
    }

    #[test]
    fn one_substitution_in_four_words_is_a_quarter() {
        assert_eq!(wer("the cat sat down", "the dog sat down"), 0.25);
    }

    #[test]
    fn diacritic_differences_count() {
        assert_eq!(wer("hôm nay trời đẹp", "hom nay trời đẹp"), 0.25);
    }

    #[test]
    fn insertions_and_deletions_count() {
        assert_eq!(wer("a b c d", "a b c d e"), 0.25);
        assert_eq!(wer("a b c d", "a b"), 0.5);
    }

    #[test]
    fn cjk_cer_counts_characters() {
        assert_eq!(cer("今天天气很好。", "今天天气很好"), 0.0);
        assert!((cer("今天天气很好", "今天天汽很好") - 1.0 / 6.0).abs() < 1e-9);
    }
}
