//! Turns raw mic/system segments (plus optional diarization turns) into a speaker-labelled transcript.
use std::collections::{HashMap, HashSet};

use unicode_normalization::UnicodeNormalization;

use super::diarize::Turn;
use super::extract::is_cjk;
use super::stt::RawSeg;
use crate::db::search::fold;
use crate::model::{Speaker, TranscriptSeg};

/// Minimum share of a mic segment a system segment must cover to count as its echo.
const ECHO_MIN_OVERLAP: f64 = 0.5;
/// Minimum word-set Jaccard similarity for an echo.
const ECHO_MIN_SIMILARITY: f64 = 0.6;
/// Minimum share of segments a language needs to be listed by `langs_of`.
const LANG_MIN_SHARE: usize = 10; // percent

fn overlap(a_start: i64, a_end: i64, b_start: i64, b_end: i64) -> i64 {
    (a_end.min(b_end) - a_start.max(b_start)).max(0)
}

fn words(text: &str) -> HashSet<String> {
    fold(text)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f64 / union as f64
}

fn is_echo(mic: &RawSeg, system: &[RawSeg]) -> bool {
    let duration = (mic.end_ms - mic.start_ms).max(1) as f64;
    let mic_words = words(&mic.text);
    system.iter().any(|s| {
        overlap(mic.start_ms, mic.end_ms, s.start_ms, s.end_ms) as f64 / duration >= ECHO_MIN_OVERLAP
            && jaccard(&mic_words, &words(&s.text)) >= ECHO_MIN_SIMILARITY
    })
}

/// The cluster of the turn overlapping `seg` most; without any overlap, the turn whose midpoint is nearest.
fn turn_speaker(seg: &RawSeg, turns: &[Turn]) -> Option<i32> {
    let best = turns
        .iter()
        .map(|t| (overlap(seg.start_ms, seg.end_ms, t.start_ms, t.end_ms), t))
        .max_by_key(|(o, _)| *o);
    match best {
        Some((o, t)) if o > 0 => Some(t.speaker),
        _ => {
            let mid = seg.start_ms + seg.end_ms;
            turns.iter().min_by_key(|t| (t.start_ms + t.end_ms - mid).abs()).map(|t| t.speaker)
        }
    }
}

/// Builds the transcript: drops mic echoes of the system track, labels mic speech `me` and system
/// speech by diarization cluster (`s1`, `s2`… by first appearance) or `others`/`s1` without turns.
pub fn assemble(
    mic: Vec<RawSeg>,
    mut system: Vec<RawSeg>,
    turns: Option<&[Turn]>,
    source: &str,
    fallback_lang: &str,
) -> (Vec<TranscriptSeg>, Vec<Speaker>) {
    system.sort_by_key(|s| s.start_ms);
    let mut labelled: Vec<(RawSeg, String)> = Vec::with_capacity(mic.len() + system.len());
    let kept_mic: Vec<RawSeg> = mic.into_iter().filter(|m| !is_echo(m, &system)).collect();
    labelled.extend(kept_mic.into_iter().map(|m| (m, "me".to_owned())));

    let mut clusters: HashMap<i32, String> = HashMap::new();
    for seg in system {
        let speaker = match turns.and_then(|t| turn_speaker(&seg, t)) {
            Some(cluster) => {
                let next = clusters.len() + 1;
                clusters.entry(cluster).or_insert_with(|| format!("s{next}")).clone()
            }
            None if source == "import" => "s1".to_owned(),
            None => "others".to_owned(),
        };
        labelled.push((seg, speaker));
    }
    labelled.sort_by_key(|(s, _)| s.start_ms);

    let mut speakers: Vec<Speaker> = Vec::new();
    if labelled.iter().any(|(_, k)| k == "me") {
        speakers.push(Speaker { key: "me".to_owned(), name: None });
    }
    let mut segs = Vec::with_capacity(labelled.len());
    for (seg, key) in labelled {
        if key != "me" && !speakers.iter().any(|s| s.key == key) {
            speakers.push(Speaker { key: key.clone(), name: None });
        }
        segs.push(TranscriptSeg {
            start_ms: seg.start_ms,
            end_ms: seg.end_ms,
            speaker: key,
            lang: guess_lang(&seg.text, fallback_lang),
            text: seg.text,
        });
    }
    (segs, speakers)
}

fn is_vietnamese(text: &str) -> bool {
    if text
        .chars()
        .any(|c| "ăâđêôơưĂÂĐÊÔƠƯ".contains(c) || ('\u{1EA0}'..='\u{1EF9}').contains(&c))
    {
        return true;
    }
    let mut prev_vowel = false;
    for c in text.nfd() {
        if matches!(c, '\u{0300}' | '\u{0301}' | '\u{0303}' | '\u{0309}' | '\u{0323}') {
            if prev_vowel {
                return true;
            }
        } else {
            prev_vowel = "aeiouyAEIOUY".contains(c);
        }
    }
    false
}

/// Best-effort language of one segment: `zh`, `vi`, `en`, else `fallback`.
pub fn guess_lang(text: &str, fallback: &str) -> String {
    if text.chars().any(is_cjk) {
        return "zh".to_owned();
    }
    if is_vietnamese(text) {
        return "vi".to_owned();
    }
    let ascii_only = text.chars().all(|c| c.is_ascii_alphabetic() || c.is_whitespace() || c.is_ascii_punctuation());
    if ascii_only && text.split_whitespace().count() >= 3 {
        return "en".to_owned();
    }
    fallback.to_owned()
}

/// Languages covering ≥ 10 % of the segments, most frequent first (ties: first appearance).
pub fn langs_of(segs: &[TranscriptSeg]) -> Vec<String> {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for seg in segs {
        match counts.iter_mut().find(|(l, _)| *l == seg.lang) {
            Some((_, n)) => *n += 1,
            None => counts.push((&seg.lang, 1)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1));
    counts
        .into_iter()
        .filter(|(_, n)| n * 100 >= segs.len() * LANG_MIN_SHARE)
        .map(|(l, _)| l.to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start_ms: i64, end_ms: i64, text: &str) -> RawSeg {
        RawSeg { start_ms, end_ms, text: text.to_owned() }
    }

    fn turn(start_ms: i64, end_ms: i64, speaker: i32) -> Turn {
        Turn { start_ms, end_ms, speaker }
    }

    fn keys(speakers: &[Speaker]) -> Vec<&str> {
        speakers.iter().map(|s| s.key.as_str()).collect()
    }

    #[test]
    fn echo_duplicate_on_mic_is_dropped() {
        let mic = vec![seg(1000, 3000, "Ok mọi người, bắt đầu nhé"), seg(5000, 6000, "Tôi đồng ý")];
        let system = vec![seg(900, 3100, "ok moi nguoi bat dau nhe")];
        let (segs, speakers) = assemble(mic, system, None, "recording", "vi");
        assert_eq!(segs.len(), 2);
        assert_eq!((segs[0].speaker.as_str(), segs[0].start_ms), ("others", 900));
        assert_eq!((segs[1].speaker.as_str(), segs[1].text.as_str()), ("me", "Tôi đồng ý"));
        assert_eq!(keys(&speakers), ["me", "others"]);
    }

    #[test]
    fn own_speech_overlapping_others_is_kept() {
        let mic = vec![seg(1000, 3000, "wait, can I add something")];
        let system = vec![seg(1000, 3000, "the release ships on friday")];
        let (segs, _) = assemble(mic, system, None, "recording", "en");
        assert_eq!(segs.len(), 2);
        assert!(segs.iter().any(|s| s.speaker == "me"));
    }

    #[test]
    fn speakers_numbered_by_first_appearance() {
        let system = vec![
            seg(0, 1000, "first"),
            seg(1000, 2000, "second"),
            seg(2000, 3000, "third"),
            seg(9000, 9500, "late"),
        ];
        let turns = [turn(0, 1100, 3), turn(1100, 2000, 1), turn(2000, 3000, 3), turn(5000, 8000, 1)];
        let (segs, speakers) = assemble(vec![], system, Some(&turns), "import", "en");
        let by_seg: Vec<&str> = segs.iter().map(|s| s.speaker.as_str()).collect();
        assert_eq!(by_seg, ["s1", "s2", "s1", "s2"]); // last one: nearest turn by midpoint
        assert_eq!(keys(&speakers), ["s1", "s2"]);
        assert!(speakers.iter().all(|s| s.name.is_none()));
    }

    #[test]
    fn no_turns_gives_others_for_recordings() {
        let make = || (vec![seg(0, 500, "hello")], vec![seg(600, 900, "hi there")]);
        let (mic, system) = make();
        let (segs, speakers) = assemble(mic, system, None, "recording", "en");
        assert_eq!(segs[1].speaker, "others");
        assert_eq!(keys(&speakers), ["me", "others"]);
        let (_, system) = make();
        let (segs, speakers) = assemble(vec![], system, None, "import", "en");
        assert_eq!(segs[0].speaker, "s1");
        assert_eq!(keys(&speakers), ["s1"]);
    }

    #[test]
    fn guess_lang_cases() {
        assert_eq!(guess_lang("Ok mọi người", "en"), "vi");
        assert_eq!(guess_lang("I can benchmark it next week", "vi"), "en");
        assert_eq!(guess_lang("我们下周", "en"), "zh");
        assert_eq!(guess_lang("API", "vi"), "vi");
        let segs: Vec<TranscriptSeg> = ["vi", "vi", "en", "vi", "vi", "vi", "vi", "vi", "vi", "vi", "vi", "zh"]
            .iter()
            .map(|l| TranscriptSeg { start_ms: 0, end_ms: 0, speaker: "me".into(), text: String::new(), lang: (*l).into() })
            .collect();
        assert_eq!(langs_of(&segs), ["vi"]);
    }
}
