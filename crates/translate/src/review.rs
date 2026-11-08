//! The pipeline: segment the pages, translate, translate back, check, and hand the rows to the
//! report. The translation service sits behind [`Translator`], so the pipeline runs the same
//! against Google and against a test double.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;

use crate::check::{self, CheckOptions};
use crate::report::Row;
use crate::segment;

/// Most strings in one request. Google's limit is 128.
const MAX_ITEMS: usize = 100;

#[derive(Debug, Clone, PartialEq)]
pub enum TranslateError {
    /// The pages hold no text.
    NoText,
    /// The run would send more characters than the limit allows.
    TooLarge {
        chars: usize,
        limit: usize,
    },
    BadLanguage(String),
    /// No API key; carries the name of the environment variable that should hold it.
    MissingKey(&'static str),
    BadKey,
    /// The model name in the environment is not usable.
    BadModel,
    /// The service answered with this HTTP status.
    Http(u16),
    /// The service could not be reached.
    Network,
    BadResponse(String),
}

impl fmt::Display for TranslateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TranslateError::NoText => f.write_str("The selected pages have no text to translate. If they are scans, run OCR first."),
            TranslateError::TooLarge { chars, limit } => write!(
                f,
                "This would send about {chars} characters to the service, above the limit of {limit}. Choose fewer pages, turn off back_check, or raise max_chars if you accept the cost."
            ),
            TranslateError::BadLanguage(c) => write!(f, "{c:?} is not a language code such as \"si\", \"en\" or \"zh-CN\"."),
            TranslateError::MissingKey(var) => write!(f, "No API key. Set the {var} environment variable."),
            TranslateError::BadKey => f.write_str("The API key has characters an API key does not use."),
            TranslateError::BadModel => f.write_str("The model name is not valid. Use a name such as gemini-3.5-flash-lite."),
            TranslateError::Http(400) => f.write_str("The service rejected the request (400). Check the language codes, the model name and the text size."),
            TranslateError::Http(403) => f.write_str(
                "The service refused the key (403). Check that the key is right, that the right API is enabled for its project (Cloud Translation API for Google Translate), and that billing or quota allows the request.",
            ),
            TranslateError::Http(429) => f.write_str(
                "The service says there were too many requests (429). Free tiers allow only a few requests a minute: wait a minute and try again, or send fewer pages.",
            ),
            TranslateError::Http(n) => write!(f, "The service returned HTTP status {n}."),
            TranslateError::Network => f.write_str("Could not reach the translation service. Check the internet connection."),
            TranslateError::BadResponse(m) => write!(f, "The service's answer was not understood: {m}."),
        }
    }
}

impl std::error::Error for TranslateError {}

/// One translated string.
#[derive(Debug, Clone, PartialEq)]
pub struct Translated {
    pub text: String,
    /// The language the service detected, when no source language was given.
    pub detected: Option<String>,
}

/// A translation service. `texts` is one request's worth; the answer has one entry per text.
pub trait Translator {
    fn translate(&mut self, texts: &[String], source: Option<&str>, target: &str) -> Result<Vec<Translated>, TranslateError>;
}

#[derive(Debug, Clone)]
pub struct Options {
    /// Source language code; `None` lets the service detect it.
    pub source: Option<String>,
    pub target: String,
    /// Translate the result back and compare it with the original.
    pub back_check: bool,
    /// Longest segment, in characters.
    pub max_segment_chars: usize,
    /// Most characters in one request. Google recommends at most 5,000.
    pub max_request_chars: usize,
    /// The run is refused when it would send more characters than this (back-translation counted).
    pub max_total_chars: usize,
    pub min_similarity: f64,
    pub target_script: Option<(u32, u32)>,
}

impl Options {
    pub fn new(target: &str) -> Self {
        Options {
            source: None,
            target: target.to_owned(),
            back_check: true,
            max_segment_chars: 3000,
            max_request_chars: 4500,
            max_total_chars: 100_000,
            // A starting guess, not a calibrated figure. Raise it to flag more rows.
            min_similarity: 0.45,
            target_script: script_of(target),
        }
    }
}

/// The code point range of the target script, for the languages where it is one block.
fn script_of(lang: &str) -> Option<(u32, u32)> {
    let primary = lang.split('-').next().unwrap_or("");
    primary.eq_ignore_ascii_case("si").then_some((0x0D80, 0x0DFF))
}

/// Whether `code` looks like a language code (`si`, `en`, `zh-CN`, `sr-Latn`).
pub fn valid_lang(code: &str) -> bool {
    if code.len() > 20 {
        return false;
    }
    let mut parts = code.split('-');
    let first = parts.next().unwrap_or("");
    (2..=3).contains(&first.len())
        && first.chars().all(|c| c.is_ascii_alphabetic())
        && parts.all(|p| (2..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphanumeric()))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Review {
    pub rows: Vec<Row>,
    /// Characters sent to the service, both directions.
    pub chars_sent: usize,
    /// The first language the service detected, when it was asked to.
    pub detected_source: Option<String>,
}

/// Translate and check `pages` (1-based page number, page text). The translation back, when
/// `opts.back_check` is on, is made by the same service.
pub fn review(pages: &[(usize, String)], tr: &mut dyn Translator, opts: &Options) -> Result<Review, TranslateError> {
    run(pages, tr, None, opts)
}

/// Like [`review`], but `checker`, a second service, makes the translation back. A mistake the
/// first service makes is then less likely to repeat in the check.
pub fn review_with_checker(
    pages: &[(usize, String)],
    tr: &mut dyn Translator,
    checker: &mut dyn Translator,
    opts: &Options,
) -> Result<Review, TranslateError> {
    run(pages, tr, Some(checker), opts)
}

fn run(pages: &[(usize, String)], tr: &mut dyn Translator, checker: Option<&mut dyn Translator>, opts: &Options) -> Result<Review, TranslateError> {
    for code in std::iter::once(&opts.target).chain(opts.source.iter()) {
        if !valid_lang(code) {
            return Err(TranslateError::BadLanguage(code.clone()));
        }
    }
    let segs = collect(pages, opts.max_segment_chars);
    if segs.is_empty() {
        return Err(TranslateError::NoText);
    }
    let total = segs.iter().map(|s| s.text.chars().count()).fold(0usize, usize::saturating_add);
    let projected = if opts.back_check { total.saturating_mul(2) } else { total };
    if projected > opts.max_total_chars {
        return Err(TranslateError::TooLarge { chars: projected, limit: opts.max_total_chars });
    }
    let texts: Vec<String> = segs.iter().map(|s| s.text.clone()).collect();
    let mut sent = 0usize;
    let forward = translate_all(tr, &texts, opts.source.as_deref(), &opts.target, opts.max_request_chars, &mut sent)?;
    let back = match (opts.back_check, checker) {
        (false, _) => vec![None; forward.len()],
        (true, Some(c)) => back_translate(c, &forward, opts, &mut sent)?,
        (true, None) => back_translate(tr, &forward, opts, &mut sent)?,
    };
    let detected_source = forward.iter().find_map(|t| t.detected.clone());
    let check_opts = CheckOptions { min_similarity: opts.min_similarity, target_script: opts.target_script };
    let rows = segs
        .into_iter()
        .zip(forward)
        .zip(back)
        .map(|((seg, fwd), back)| {
            let checked = check::check(&seg.text, &fwd.text, back.as_deref(), &check_opts);
            Row {
                page: seg.page,
                index: seg.index,
                source: seg.text,
                translated: fwd.text,
                back,
                similarity: checked.similarity,
                flags: checked.flags,
            }
        })
        .collect();
    Ok(Review { rows, chars_sent: sent, detected_source })
}

struct Seg {
    page: usize,
    index: usize,
    text: String,
}

fn collect(pages: &[(usize, String)], max_chars: usize) -> Vec<Seg> {
    let mut out = Vec::new();
    for (page, text) in pages {
        for (i, s) in segment::segments(text, max_chars).into_iter().enumerate() {
            out.push(Seg { page: *page, index: i.saturating_add(1), text: s });
        }
    }
    out
}

/// Translate each segment's result back into the language it came from. Segments whose source
/// language is unknown, undetermined or already the target are left unchecked.
fn back_translate(tr: &mut dyn Translator, forward: &[Translated], opts: &Options, sent: &mut usize) -> Result<Vec<Option<String>>, TranslateError> {
    let mut back: Vec<Option<String>> = vec![None; forward.len()];
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, t) in forward.iter().enumerate() {
        let lang = opts.source.clone().or_else(|| t.detected.clone());
        if let Some(l) = lang.filter(|l| valid_lang(l) && !l.eq_ignore_ascii_case("und") && !l.eq_ignore_ascii_case(&opts.target)) {
            groups.entry(l).or_default().push(i);
        }
    }
    for (lang, idx) in groups {
        let texts: Vec<String> = idx.iter().filter_map(|i| forward.get(*i)).map(|t| t.text.clone()).collect();
        let got = translate_all(tr, &texts, Some(&opts.target), &lang, opts.max_request_chars, sent)?;
        for (i, g) in idx.into_iter().zip(got) {
            if let Some(slot) = back.get_mut(i) {
                *slot = Some(g.text);
            }
        }
    }
    Ok(back)
}

fn translate_all(
    tr: &mut dyn Translator,
    texts: &[String],
    source: Option<&str>,
    target: &str,
    max_chars: usize,
    sent: &mut usize,
) -> Result<Vec<Translated>, TranslateError> {
    let mut out = Vec::with_capacity(texts.len());
    for range in batches(texts, max_chars, MAX_ITEMS) {
        let batch = texts.get(range).ok_or_else(|| TranslateError::BadResponse("internal batching error".into()))?;
        *sent = sent.saturating_add(batch.iter().map(|t| t.chars().count()).fold(0usize, usize::saturating_add));
        let got = tr.translate(batch, source, target)?;
        if got.len() != batch.len() {
            return Err(TranslateError::BadResponse(format!("expected {} translations, got {}", batch.len(), got.len())));
        }
        out.extend(got);
    }
    Ok(out)
}

/// Index ranges of `texts`, each within `max_chars` characters and `max_items` strings. A single
/// text longer than `max_chars` gets a range of its own.
pub(crate) fn batches(texts: &[String], max_chars: usize, max_items: usize) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut chars = 0usize;
    for (i, t) in texts.iter().enumerate() {
        let n = t.chars().count();
        let full = i > start && (chars.saturating_add(n) > max_chars || i - start >= max_items);
        if full {
            out.push(start..i);
            start = i;
            chars = 0;
        }
        chars = chars.saturating_add(n);
    }
    if start < texts.len() {
        out.push(start..texts.len());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A translator that applies `f` to every text and records each request.
    struct Fake<F: FnMut(&str, Option<&str>, &str) -> String> {
        f: F,
        detect: Option<&'static str>,
        requests: Vec<(Option<String>, String, usize)>,
    }

    fn fake<F: FnMut(&str, Option<&str>, &str) -> String>(f: F) -> Fake<F> {
        Fake { f, detect: Some("en"), requests: Vec::new() }
    }

    impl<F: FnMut(&str, Option<&str>, &str) -> String> Translator for Fake<F> {
        fn translate(&mut self, texts: &[String], source: Option<&str>, target: &str) -> Result<Vec<Translated>, TranslateError> {
            self.requests.push((source.map(str::to_owned), target.to_owned(), texts.len()));
            let detected = if source.is_none() { self.detect.map(str::to_owned) } else { None };
            Ok(texts.iter().map(|t| Translated { text: (self.f)(t, source, target), detected: detected.clone() }).collect())
        }
    }

    /// Forward: prefix with a Sinhala word. Back: take the prefix off again.
    fn round_trip(text: &str, _source: Option<&str>, target: &str) -> String {
        if target == "si" { format!("ආයුබෝවන් {text}") } else { text.trim_start_matches("ආයුබෝවන් ").to_owned() }
    }

    fn pages() -> Vec<(usize, String)> {
        vec![(1, "The invoice is due on 5 May.\n\nPay at https://pay.example/x".into()), (3, "Thank you for your order.".into())]
    }

    #[test]
    fn a_clean_round_trip_gives_rows_with_no_flags() {
        let mut tr = fake(round_trip);
        let r = review(&pages(), &mut tr, &Options::new("si")).unwrap();
        assert_eq!(r.rows.len(), 3);
        assert!(r.rows.iter().all(|row| row.flags.is_empty()), "{:?}", r.rows);
        assert!(r.rows.iter().all(|row| row.similarity == Some(1.0)));
        assert_eq!((r.rows[0].page, r.rows[0].index), (1, 1));
        assert_eq!((r.rows[2].page, r.rows[2].index), (3, 1));
        assert_eq!(r.detected_source.as_deref(), Some("en"));
    }

    #[test]
    fn the_back_translation_goes_to_the_detected_language() {
        let mut tr = fake(round_trip);
        review(&pages(), &mut tr, &Options::new("si")).unwrap();
        assert_eq!(tr.requests[0], (None, "si".to_owned(), 3));
        assert_eq!(tr.requests[1], (Some("si".to_owned()), "en".to_owned(), 3));
    }

    #[test]
    fn a_second_service_can_make_the_translation_back() {
        let mut first = fake(round_trip);
        let mut checker = fake(round_trip);
        let r = review_with_checker(&pages(), &mut first, &mut checker, &Options::new("si")).unwrap();
        assert_eq!(first.requests, vec![(None, "si".to_owned(), 3)], "the first service only translates forward");
        assert_eq!(checker.requests, vec![(Some("si".to_owned()), "en".to_owned(), 3)], "the second one checks, into the detected language");
        assert!(r.rows.iter().all(|row| row.flags.is_empty() && row.similarity == Some(1.0)));
    }

    #[test]
    fn the_second_service_is_not_used_when_back_check_is_off() {
        let mut first = fake(round_trip);
        let mut checker = fake(round_trip);
        let mut o = Options::new("si");
        o.back_check = false;
        review_with_checker(&pages(), &mut first, &mut checker, &o).unwrap();
        assert!(checker.requests.is_empty());
    }

    #[test]
    fn a_given_source_language_is_used_both_ways() {
        let mut tr = fake(round_trip);
        let mut o = Options::new("si");
        o.source = Some("en".into());
        review(&pages(), &mut tr, &o).unwrap();
        assert_eq!(tr.requests[0], (Some("en".to_owned()), "si".to_owned(), 3));
        assert_eq!(tr.requests[1], (Some("si".to_owned()), "en".to_owned(), 3));
    }

    #[test]
    fn a_number_dropped_in_translation_is_flagged() {
        let mut tr = fake(|t, s, target| round_trip(t, s, target).replace('5', ""));
        let r = review(&pages(), &mut tr, &Options::new("si")).unwrap();
        assert!(r.rows[0].flags.iter().any(|f| matches!(f, check::Flag::NumbersDiffer { .. })), "{:?}", r.rows[0]);
        assert!(r.rows[2].flags.is_empty());
    }

    #[test]
    fn a_back_translation_that_drifts_is_flagged() {
        let mut tr = fake(|t, _, target| if target == "si" { format!("ආයුබෝවන් {t}") } else { "Cats like warm sunny windows".to_owned() });
        let r = review(&pages(), &mut tr, &Options::new("si")).unwrap();
        assert!(r.rows.iter().all(|row| row.flags.iter().any(|f| matches!(f, check::Flag::LowBackSimilarity(_)))), "{:?}", r.rows);
    }

    #[test]
    fn without_back_check_there_is_one_pass_and_no_similarity() {
        let mut tr = fake(round_trip);
        let mut o = Options::new("si");
        o.back_check = false;
        let r = review(&pages(), &mut tr, &o).unwrap();
        assert_eq!(tr.requests.len(), 1);
        assert!(r.rows.iter().all(|row| row.back.is_none() && row.similarity.is_none()));
    }

    #[test]
    fn text_already_in_the_target_language_is_not_sent_back() {
        let mut tr = fake(round_trip);
        tr.detect = Some("si");
        let r = review(&pages(), &mut tr, &Options::new("si")).unwrap();
        assert_eq!(tr.requests.len(), 1, "no back-translation into the same language");
        assert!(r.rows.iter().all(|row| row.back.is_none()));
    }

    #[test]
    fn an_undetermined_language_is_not_checked_back() {
        let mut tr = fake(round_trip);
        tr.detect = Some("und");
        review(&pages(), &mut tr, &Options::new("si")).unwrap();
        assert_eq!(tr.requests.len(), 1);
    }

    #[test]
    fn a_run_over_the_limit_is_refused_before_anything_is_sent() {
        let mut tr = fake(round_trip);
        let mut o = Options::new("si");
        o.max_total_chars = 50;
        let e = review(&pages(), &mut tr, &o).unwrap_err();
        assert!(matches!(e, TranslateError::TooLarge { limit: 50, .. }), "{e:?}");
        assert!(tr.requests.is_empty());
    }

    #[test]
    fn back_translation_counts_toward_the_limit() {
        let text = vec![(1, "x".repeat(60))];
        let mut o = Options::new("si");
        o.max_total_chars = 100;
        assert!(matches!(review(&text, &mut fake(round_trip), &o), Err(TranslateError::TooLarge { chars: 120, .. })));
        o.back_check = false;
        assert!(review(&text, &mut fake(round_trip), &o).is_ok());
    }

    #[test]
    fn no_text_and_bad_language_codes_are_refused() {
        let mut tr = fake(round_trip);
        assert_eq!(review(&[(1, " \n ".into())], &mut tr, &Options::new("si")), Err(TranslateError::NoText));
        assert_eq!(review(&pages(), &mut tr, &Options::new("not a code")), Err(TranslateError::BadLanguage("not a code".into())));
        let mut o = Options::new("si");
        o.source = Some("en&target=xx".into());
        assert!(matches!(review(&pages(), &mut tr, &o), Err(TranslateError::BadLanguage(_))));
        assert!(tr.requests.is_empty());
    }

    #[test]
    fn a_service_that_returns_the_wrong_number_of_answers_is_an_error() {
        struct Short;
        impl Translator for Short {
            fn translate(&mut self, _: &[String], _: Option<&str>, _: &str) -> Result<Vec<Translated>, TranslateError> {
                Ok(vec![])
            }
        }
        let e = review(&pages(), &mut Short, &Options::new("si")).unwrap_err();
        assert!(matches!(e, TranslateError::BadResponse(_)), "{e:?}");
    }

    #[test]
    fn a_service_error_stops_the_run_with_that_error() {
        struct Down;
        impl Translator for Down {
            fn translate(&mut self, _: &[String], _: Option<&str>, _: &str) -> Result<Vec<Translated>, TranslateError> {
                Err(TranslateError::Http(403))
            }
        }
        assert_eq!(review(&pages(), &mut Down, &Options::new("si")), Err(TranslateError::Http(403)));
    }

    #[test]
    fn requests_are_batched_within_the_character_and_item_limits() {
        let t = |n: usize| "a".repeat(n);
        assert_eq!(batches(&[t(10), t(10), t(10)], 25, 100), vec![0..2, 2..3]);
        assert_eq!(batches(&[t(10), t(10), t(10)], 100, 2), vec![0..2, 2..3]);
        assert_eq!(batches(&[t(50), t(5)], 20, 100), vec![0..1, 1..2], "an oversized text gets its own request");
        assert!(batches(&[], 10, 10).is_empty());
    }

    #[test]
    fn language_codes_are_checked() {
        for ok in ["si", "en", "zh-CN", "sr-Latn", "fil", "pt-BR"] {
            assert!(valid_lang(ok), "{ok}");
        }
        for bad in ["", "s", "english", "en_US", "en-", "en-x", "si&x=1", "en-12345678901234567890"] {
            assert!(!valid_lang(bad), "{bad}");
        }
    }

    #[test]
    fn the_sinhala_target_gets_a_script_check_and_others_do_not() {
        assert_eq!(Options::new("si").target_script, Some((0x0D80, 0x0DFF)));
        assert_eq!(Options::new("SI").target_script, Some((0x0D80, 0x0DFF)));
        assert_eq!(Options::new("ta").target_script, None);
    }

    #[test]
    fn errors_read_as_actions() {
        assert!(TranslateError::MissingKey("PDFCRAFT_SOME_KEY").to_string().contains("PDFCRAFT_SOME_KEY"));
        assert!(TranslateError::Http(429).to_string().contains("wait a minute"));
        assert!(TranslateError::BadModel.to_string().contains("model name"));
        assert!(TranslateError::Http(403).to_string().contains("billing"));
        assert!(TranslateError::NoText.to_string().contains("OCR"));
    }
}
