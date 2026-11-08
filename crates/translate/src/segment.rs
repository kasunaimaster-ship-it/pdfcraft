//! Splits page text into segments small enough for one translation request.
//!
//! Text from a PDF arrives as hard-wrapped lines. Lines in one block (no blank line between
//! them) are joined with a space so the translator sees whole sentences. A block longer than the
//! limit is cut after sentence-ending marks, and only as a last resort in the middle of a run.

/// Sentence-ending marks across the scripts the tool is likely to meet.
const TERMINATORS: &[char] = &['.', '!', '?', '。', '！', '？', '।', '॥', '؟', '።'];

/// Segments of at most `max_chars` characters (counted as Unicode scalar values, at least 1).
pub fn segments(page_text: &str, max_chars: usize) -> Vec<String> {
    let max = max_chars.max(1);
    let mut out = Vec::new();
    for block in blocks(page_text) {
        if block.chars().count() <= max {
            out.push(block);
        } else {
            split_long(&block, max, &mut out);
        }
    }
    out
}

fn blocks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut lines: Vec<&str> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            flush_block(&mut lines, &mut out);
        } else {
            lines.push(line);
        }
    }
    flush_block(&mut lines, &mut out);
    out
}

fn flush_block(lines: &mut Vec<&str>, out: &mut Vec<String>) {
    if !lines.is_empty() {
        out.push(lines.join(" "));
        lines.clear();
    }
}

fn sentences(block: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in block.chars() {
        cur.push(c);
        if TERMINATORS.contains(&c) {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn split_long(block: &str, max: usize, out: &mut Vec<String>) {
    let mut cur = String::new();
    let mut cur_len = 0usize;
    for piece in sentences(block) {
        let n = piece.chars().count();
        if n > max {
            push_trimmed(out, &mut cur);
            cur_len = 0;
            hard_split(&piece, max, out);
            continue;
        }
        if cur_len.saturating_add(n) > max {
            push_trimmed(out, &mut cur);
            cur_len = 0;
        }
        cur.push_str(&piece);
        cur_len = cur_len.saturating_add(n);
    }
    push_trimmed(out, &mut cur);
}

fn push_trimmed(out: &mut Vec<String>, cur: &mut String) {
    let t = cur.trim();
    if !t.is_empty() {
        out.push(t.to_owned());
    }
    cur.clear();
}

fn hard_split(piece: &str, max: usize, out: &mut Vec<String>) {
    let chars: Vec<char> = piece.chars().collect();
    // `max` is at least 1, so `chunks` cannot panic.
    for chunk in chars.chunks(max) {
        let s: String = chunk.iter().collect();
        let t = s.trim();
        if !t.is_empty() {
            out.push(t.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_lines_separate_blocks_and_lines_in_a_block_are_joined() {
        let got = segments("First line\nsecond line\n\n\nNext block\n", 100);
        assert_eq!(got, vec!["First line second line".to_owned(), "Next block".to_owned()]);
    }

    #[test]
    fn nothing_in_nothing_out() {
        assert!(segments("", 10).is_empty());
        assert!(segments("  \n\t\n \n", 10).is_empty());
    }

    #[test]
    fn a_long_block_is_cut_after_sentences_and_stays_under_the_limit() {
        let text = "One two three. Four five six. Seven eight nine. Ten eleven twelve.";
        let got = segments(text, 32);
        assert!(got.len() > 1, "{got:?}");
        assert!(got.iter().all(|s| s.chars().count() <= 32), "{got:?}");
        let keep = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        assert_eq!(keep(&got.join(" ")), keep(text), "no text may be lost");
        assert!(got[0].ends_with('.'), "cut at a sentence end: {got:?}");
    }

    #[test]
    fn a_run_without_sentence_marks_is_cut_by_characters_and_never_inside_one() {
        let text = "සිංහල".repeat(40);
        let got = segments(&text, 7);
        assert!(got.iter().all(|s| s.chars().count() <= 7), "{got:?}");
        assert_eq!(got.concat(), text);
    }

    #[test]
    fn a_zero_limit_does_not_panic() {
        let got = segments("abc def", 0);
        assert_eq!(got.concat(), "abcdef");
    }

    #[test]
    fn cjk_sentence_marks_are_boundaries() {
        let got = segments("今日は晴れです。明日は雨です。明後日は曇りです。", 10);
        assert!(got.iter().all(|s| s.chars().count() <= 10), "{got:?}");
        assert!(got.iter().all(|s| s.ends_with('。')), "{got:?}");
    }
}
