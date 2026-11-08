//! Automatic checks on one translated segment.
//!
//! These find some kinds of problem; they never prove a translation right. Every flag is advice
//! for the person reading the review, not a verdict.

use std::collections::HashMap;

/// At most this many differing items are named in one flag.
const MAX_LISTED: usize = 10;

/// Code points of the digit zero in the numeral systems a source document may use.
const DIGIT_ZEROES: [u32; 9] = [0x30, 0x660, 0x6F0, 0x966, 0x9E6, 0xBE6, 0xDE6, 0xE50, 0xFF10];

#[derive(Debug, Clone, PartialEq)]
pub enum Flag {
    /// The service returned nothing for a segment that has text.
    Empty,
    /// The translation is the original text again.
    Untranslated,
    /// The translation has no letters in the target script.
    WrongScript,
    /// Numbers present on one side only.
    NumbersDiffer { missing: Vec<String>, extra: Vec<String> },
    /// Web addresses or e-mail addresses present on one side only.
    LinksDiffer { missing: Vec<String>, extra: Vec<String> },
    /// Translating back gave a text unlike the original (similarity 0 to 1).
    LowBackSimilarity(f64),
}

impl Flag {
    /// A sentence for the review table.
    pub fn describe(&self) -> String {
        match self {
            Flag::Empty => "The translation is empty.".to_owned(),
            Flag::Untranslated => "The text looks unchanged. It may not have been translated.".to_owned(),
            Flag::WrongScript => "The translation has no letters in the target script.".to_owned(),
            Flag::NumbersDiffer { missing, extra } => differ("Numbers differ.", missing, extra),
            Flag::LinksDiffer { missing, extra } => differ("Web or e-mail addresses differ.", missing, extra),
            Flag::LowBackSimilarity(s) => {
                format!("Translating back gives a text only {:.0}% similar to the original. The meaning may have changed.", s * 100.0)
            }
        }
    }
}

fn differ(head: &str, missing: &[String], extra: &[String]) -> String {
    let mut s = head.to_owned();
    if !missing.is_empty() {
        s.push_str(&format!(" Missing from the translation: {}.", missing.join(", ")));
    }
    if !extra.is_empty() {
        s.push_str(&format!(" Not in the original: {}.", extra.join(", ")));
    }
    s
}

#[derive(Debug, Clone, Copy)]
pub struct CheckOptions {
    /// Back-translation similarity below this raises [`Flag::LowBackSimilarity`].
    pub min_similarity: f64,
    /// Inclusive code point range of the target script, when it can be checked.
    pub target_script: Option<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    pub flags: Vec<Flag>,
    /// Similarity of the original and its back-translation, when there is one.
    pub similarity: Option<f64>,
}

/// Check `translated` against `source`, and against `back` (the translation translated back).
pub fn check(source: &str, translated: &str, back: Option<&str>, opts: &CheckOptions) -> Checked {
    let has_text = source.chars().any(char::is_alphanumeric);
    if has_text && translated.trim().is_empty() {
        return Checked { flags: vec![Flag::Empty], similarity: None };
    }
    let mut flags = Vec::new();
    let unchanged = untranslated(source, translated);
    if unchanged {
        flags.push(Flag::Untranslated);
    } else if wrong_script(source, translated, opts.target_script) {
        flags.push(Flag::WrongScript);
    }
    let (missing_n, extra_n) = both_ways(&digit_runs(source), &digit_runs(translated));
    if !missing_n.is_empty() || !extra_n.is_empty() {
        flags.push(Flag::NumbersDiffer { missing: missing_n, extra: extra_n });
    }
    let (missing_l, extra_l) = both_ways(&links(source), &links(translated));
    if !missing_l.is_empty() || !extra_l.is_empty() {
        flags.push(Flag::LinksDiffer { missing: missing_l, extra: extra_l });
    }
    let similarity = back.map(|b| similarity(source, b));
    if let Some(s) = similarity
        && s < opts.min_similarity
    {
        flags.push(Flag::LowBackSimilarity(s));
    }
    Checked { flags, similarity }
}

fn both_ways(source: &[String], target: &[String]) -> (Vec<String>, Vec<String>) {
    let mut lost = missing(source, target);
    let mut added = missing(target, source);
    lost.truncate(MAX_LISTED);
    added.truncate(MAX_LISTED);
    (lost, added)
}

fn untranslated(source: &str, translated: &str) -> bool {
    let letters = source.chars().filter(|c| c.is_alphabetic()).count();
    letters >= 3 && normalized(source) == normalized(translated)
}

fn wrong_script(source: &str, translated: &str, script: Option<(u32, u32)>) -> bool {
    let Some((lo, hi)) = script else { return false };
    let in_script = |s: &str| s.chars().any(|c| (lo..=hi).contains(&(c as u32)));
    let has_letters = |s: &str| s.chars().any(char::is_alphabetic);
    has_letters(source) && has_letters(translated) && !in_script(source) && !in_script(translated)
}

fn digit_value(c: char) -> Option<u32> {
    let cp = c as u32;
    DIGIT_ZEROES.iter().find_map(|zero| {
        let v = cp.wrapping_sub(*zero);
        (v < 10).then_some(v)
    })
}

/// The runs of digits in `s`, written with ASCII digits whatever numeral system they came in.
pub fn digit_runs(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        match digit_value(c).and_then(|v| char::from_digit(v, 10)) {
            Some(d) => cur.push(d),
            None => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Web and e-mail addresses in `s`, lower-cased.
pub fn links(s: &str) -> Vec<String> {
    s.split_whitespace().filter_map(link_in).collect()
}

fn link_in(token: &str) -> Option<String> {
    let t = token.trim_start_matches(['(', '[', '<', '"', '\'']);
    let ascii: String = t.chars().take_while(char::is_ascii_graphic).collect();
    let ascii = ascii.trim_end_matches(['.', ',', ';', ':', ')', ']', '>', '"', '\'', '!', '?']);
    let lower = ascii.to_ascii_lowercase();
    let url = lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("www.");
    (url || email_like(&lower)).then_some(lower)
}

fn email_like(t: &str) -> bool {
    match t.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.') && !domain.contains('@')
        }
        None => false,
    }
}

/// The items of `source` that `target` does not have, counting repeats.
pub fn missing(source: &[String], target: &[String]) -> Vec<String> {
    let mut pool: Vec<Option<&String>> = target.iter().map(Some).collect();
    let mut out = Vec::new();
    for s in source {
        match pool.iter_mut().find(|slot| slot.is_some_and(|t| t == s)) {
            Some(slot) => *slot = None,
            None => out.push(s.clone()),
        }
    }
    out
}

fn normalized(s: &str) -> Vec<char> {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn bigrams(c: &[char]) -> HashMap<(char, char), usize> {
    let mut m = HashMap::new();
    for w in c.windows(2) {
        if let [x, y] = w {
            *m.entry((*x, *y)).or_insert(0usize) += 1;
        }
    }
    m
}

/// Overlap of the letter pairs of `a` and `b`, from 0 (nothing shared) to 1 (the same letters).
/// It works on any script, including ones written without spaces.
pub fn similarity(a: &str, b: &str) -> f64 {
    let (na, nb) = (normalized(a), normalized(b));
    if na == nb {
        return 1.0;
    }
    let (ga, gb) = (bigrams(&na), bigrams(&nb));
    let total = na.len().saturating_sub(1).saturating_add(nb.len().saturating_sub(1));
    if total == 0 {
        return 0.0;
    }
    let shared: usize = ga.iter().map(|(k, v)| (*v).min(gb.get(k).copied().unwrap_or(0))).sum();
    (2 * shared) as f64 / total as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    const SI: Option<(u32, u32)> = Some((0x0D80, 0x0DFF));

    fn opts() -> CheckOptions {
        CheckOptions { min_similarity: 0.45, target_script: SI }
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn digits_are_compared_across_numeral_systems() {
        assert_eq!(digit_runs("Room 12, floor 3"), strs(&["12", "3"]));
        assert_eq!(digit_runs("කාමරය ෧෨"), strs(&["12"]), "Sinhala Lith digits");
        assert_eq!(digit_runs("١٢٣ and ４５"), strs(&["123", "45"]), "Arabic-Indic and full-width");
        assert!(digit_runs("no digits").is_empty());
    }

    #[test]
    fn a_number_lost_or_added_is_flagged_with_the_numbers_named() {
        let c = check("Pay 1500 by 2026", "ගෙවන්න 150 2026", None, &opts());
        assert!(c.flags.contains(&Flag::NumbersDiffer { missing: strs(&["1500"]), extra: strs(&["150"]) }), "{c:?}");
    }

    #[test]
    fn the_same_numbers_in_another_numeral_system_are_not_flagged() {
        let c = check("Page 12 of 40", "පිටුව ෧෨ / ෪෦", None, &opts());
        assert!(c.flags.is_empty(), "{c:?}");
    }

    #[test]
    fn repeated_numbers_are_counted() {
        let c = check("1 and 1", "1", None, &opts());
        assert!(c.flags.iter().any(|f| matches!(f, Flag::NumbersDiffer { .. })), "{c:?}");
    }

    #[test]
    fn links_are_found_trimmed_and_compared() {
        assert_eq!(links("See (https://Example.com/a). Mail: me@x.org, or www.y.lk!"), strs(&["https://example.com/a", "me@x.org", "www.y.lk"]));
        let c = check("Visit https://a.example/x now", "දැන් https://b.example/x බලන්න", None, &opts());
        assert!(c.flags.iter().any(|f| matches!(f, Flag::LinksDiffer { .. })), "{c:?}");
        let ok = check("Visit https://a.example/x now", "දැන් https://a.example/x බලන්න", None, &opts());
        assert!(ok.flags.is_empty(), "{ok:?}");
    }

    #[test]
    fn a_link_followed_by_non_ascii_text_stops_at_the_link() {
        assert_eq!(links("https://a.example/xඅවසානය"), strs(&["https://a.example/x"]));
    }

    #[test]
    fn similarity_is_one_for_the_same_letters_and_low_for_unrelated_text() {
        assert!((similarity("Hello, World", "hello world") - 1.0).abs() < f64::EPSILON);
        assert!(similarity("The invoice is due on Friday", "Cats like warm sunny windows") < 0.3);
        assert!(similarity("The invoice is due on Friday", "The invoice is due Friday") > 0.8);
        assert!(similarity("今日は晴れです", "今日は雨です") > 0.3, "scripts without spaces still compare");
    }

    #[test]
    fn similarity_never_panics_on_tiny_or_empty_input() {
        assert!((similarity("", "") - 1.0).abs() < f64::EPSILON);
        assert!(similarity("a", "b").abs() < f64::EPSILON);
        assert!(similarity("", "abc").abs() < f64::EPSILON);
    }

    #[test]
    fn a_back_translation_that_drifted_is_flagged() {
        let bad = check("The contract ends on 5 May.", "ගිවිසුම අවසන් වේ 5", Some("Cats like warm windows 5"), &opts());
        assert!(bad.flags.iter().any(|f| matches!(f, Flag::LowBackSimilarity(_))), "{bad:?}");
        assert!(bad.similarity.is_some());
        let good = check("The contract ends on 5 May.", "ගිවිසුම අවසන් වේ 5", Some("The contract ends on 5 May"), &opts());
        assert!(good.flags.is_empty(), "{good:?}");
    }

    #[test]
    fn empty_unchanged_and_wrong_script_translations_are_flagged() {
        assert_eq!(check("Hello there", "  ", None, &opts()).flags, vec![Flag::Empty]);
        assert_eq!(check("Hello there", "hello THERE", None, &opts()).flags, vec![Flag::Untranslated]);
        assert_eq!(check("Hello there", "Bonjour tout le monde", None, &opts()).flags, vec![Flag::WrongScript]);
        assert!(check("Hello there", "ආයුබෝවන්", None, &opts()).flags.is_empty());
    }

    #[test]
    fn text_that_is_already_in_the_target_script_is_not_flagged_as_wrong() {
        assert!(check("ආයුබෝවන්", "ආයුබෝවන්", None, &opts()).flags.iter().all(|f| *f != Flag::WrongScript));
    }

    #[test]
    fn a_source_with_no_letters_is_never_flagged_untranslated() {
        assert!(check("2026-11-08", "2026-11-08", None, &opts()).flags.is_empty());
    }

    #[test]
    fn flags_read_as_sentences() {
        let f = Flag::NumbersDiffer { missing: strs(&["12"]), extra: strs(&["21"]) };
        assert_eq!(f.describe(), "Numbers differ. Missing from the translation: 12. Not in the original: 21.");
        assert!(Flag::LowBackSimilarity(0.31).describe().contains("31%"));
    }

    #[test]
    fn long_lists_are_cut() {
        let src: String = (10..40).map(|n| format!("{n} ")).collect();
        let c = check(&src, "x", None, &opts());
        match &c.flags[..] {
            [.., Flag::NumbersDiffer { missing, .. }] => assert_eq!(missing.len(), MAX_LISTED),
            other => panic!("{other:?}"),
        }
    }
}
