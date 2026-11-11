//! The review table: one HTML page, no scripts, that opens in any browser.

use std::fmt::Write;

use crate::check::Flag;

/// One checked segment.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// 1-based page the text came from.
    pub page: usize,
    /// 1-based position of the segment on its page.
    pub index: usize,
    pub source: String,
    pub translated: String,
    pub back: Option<String>,
    pub similarity: Option<f64>,
    pub flags: Vec<Flag>,
}

/// What the page says about the run.
#[derive(Debug, Clone, Copy)]
pub struct Meta<'a> {
    pub document: &'a str,
    /// A language code, or `auto` when the service detected it.
    pub source_lang: &'a str,
    pub target_lang: &'a str,
    pub engine: &'a str,
    pub back_checked: bool,
}

pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

const STYLE: &str = r#"
:root { --bg: #f6f7f7; --fg: #17201f; --muted: #5b6b69; --line: #d3dcda; --card: #ffffff; --flag: #fff1d6; --flag-line: #c98a1b; }
@media (prefers-color-scheme: dark) { :root { --bg: #101615; --fg: #e4ecea; --muted: #94a6a3; --line: #2b3a38; --card: #172120; --flag: #3a2e12; --flag-line: #d9a441; color-scheme: dark; } }
body { margin: 0; background: var(--bg); color: var(--fg); font: 15px/1.55 system-ui, sans-serif; }
main { max-width: 1200px; margin: 0 auto; padding: 24px 16px 64px; }
h1 { margin: 0 0 4px; font-size: 1.5rem; }
.meta, .note { color: var(--muted); margin: 4px 0; }
.summary { font-weight: 600; margin: 16px 0; }
.wrap { overflow-x: auto; border: 1px solid var(--line); border-radius: 6px; background: var(--card); }
table { border-collapse: collapse; width: 100%; min-width: 760px; }
th, td { text-align: left; vertical-align: top; padding: 10px 12px; border-bottom: 1px solid var(--line); }
th { color: var(--muted); font-size: 0.85rem; font-weight: 600; }
td.text { white-space: pre-wrap; overflow-wrap: anywhere; }
td.si, td[lang] { font-family: "Noto Sans Sinhala", "Iskoola Pota", "Nirmala UI", system-ui, sans-serif; }
td.pos { white-space: nowrap; color: var(--muted); font-variant-numeric: tabular-nums; }
tr.flagged td { background: var(--flag); }
tr.flagged td.pos { border-left: 4px solid var(--flag-line); }
ul.checks { margin: 0; padding-left: 18px; }
.ok { color: var(--muted); }
.doc { background: var(--card); border: 1px solid var(--line); border-radius: 6px; padding: 8px 20px 20px; margin: 16px 0 28px; font-family: "Noto Sans Sinhala", "Iskoola Pota", "Nirmala UI", system-ui, sans-serif; font-size: 16px; line-height: 1.7; }
.doc h2 { margin: 12px 0 4px; font-size: 1.25rem; }
.doc h3 { margin: 22px 0 6px; font-size: 0.9rem; font-weight: 600; color: var(--muted); }
.doc p.seg { margin: 0 0 10px; white-space: pre-wrap; overflow-wrap: anywhere; }
.doc p.seg.flagged { background: var(--flag); }
.howto { padding: 10px 12px; border: 1px solid var(--line); border-radius: 6px; background: var(--card); }
@media print {
  body { background: #fff; color: #000; font-size: 12pt; }
  main { max-width: none; padding: 0; }
  .screen, .review { display: none !important; }
  .doc { border: 0; padding: 0; margin: 0; background: none; }
  .doc h3 { break-after: avoid; }
  .doc section + section { break-before: page; }
  .doc p.seg.flagged { background: none; }
  .doc p.seg { break-inside: avoid; }
}
"#;

/// The review page for `rows`.
pub fn html(meta: &Meta, rows: &[Row]) -> String {
    let flagged = rows.iter().filter(|r| !r.flags.is_empty()).count();
    let mut s = String::new();
    s.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    s.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>Translation review</title>\n");
    let _ = write!(s, "<style>{STYLE}</style>\n</head>\n<body>\n<main>\n");
    let _ = writeln!(s, "<h1 class=\"screen\">Translation review</h1>");
    let _ = writeln!(
        s,
        "<p class=\"meta screen\">{} · {} to {} · {}</p>",
        escape_html(meta.document),
        escape_html(meta.source_lang),
        escape_html(meta.target_lang),
        escape_html(meta.engine)
    );
    s.push_str(
        "<p class=\"note screen\">This is a machine translation. The automatic checks find some problems (numbers, addresses, text left \
         untranslated, a meaning that drifts) but cannot show that a translation is right. A row without a note has not been \
         verified. Have someone who reads both languages check anything important.</p>\n",
    );
    if !meta.back_checked {
        s.push_str("<p class=\"note screen\">Back-translation was off, so meaning drift was not checked.</p>\n");
    }
    let _ = writeln!(s, "<p class=\"summary screen\">{} segments, {} with notes to check.</p>", rows.len(), flagged);
    document(&mut s, meta, rows);
    s.push_str("<section class=\"review\">\n<h2>Review table</h2>\n");
    s.push_str("<div class=\"wrap\"><table>\n<thead><tr><th>Page</th><th>Original</th><th>Translation</th>");
    if meta.back_checked {
        s.push_str("<th>Translated back</th>");
    }
    s.push_str("<th>Checks</th></tr></thead>\n<tbody>\n");
    for r in rows {
        row(&mut s, meta, r);
    }
    s.push_str("</tbody>\n</table></div>\n</section>\n</main>\n</body>\n</html>\n");
    s
}

/// The translation alone, page by page. When the page is printed only this is printed, so the
/// browser's "Save as PDF" makes a PDF of the translation; the table and the notes stay on screen.
fn document(s: &mut String, meta: &Meta, rows: &[Row]) {
    let _ = writeln!(s, "<div class=\"doc\" lang=\"{}\">", escape_html(meta.target_lang));
    let _ = writeln!(s, "<h2>{}</h2>", escape_html(meta.document));
    s.push_str(
        "<p class=\"howto screen\">To save this translation as a PDF: open the browser menu, choose Print (or Share, then Print), \
         and pick Save as PDF as the printer. Only the translation is printed. Yellow paragraphs have automatic notes in the \
         table below.</p>\n",
    );
    let mut page = None;
    for r in rows {
        if page != Some(r.page) {
            if page.is_some() {
                s.push_str("</section>\n");
            }
            let _ = writeln!(s, "<section>\n<h3>Page {}</h3>", r.page);
            page = Some(r.page);
        }
        let class = if r.flags.is_empty() { "seg" } else { "seg flagged" };
        let _ = writeln!(s, "<p class=\"{class}\">{}</p>", escape_html(&r.translated));
    }
    if page.is_some() {
        s.push_str("</section>\n");
    }
    s.push_str("</div>\n");
}

fn row(s: &mut String, meta: &Meta, r: &Row) {
    let class = if r.flags.is_empty() { "" } else { " class=\"flagged\"" };
    let _ = write!(s, "<tr{class}><td class=\"pos\">{}.{}</td>", r.page, r.index);
    let _ = write!(s, "<td class=\"text\">{}</td>", escape_html(&r.source));
    let _ = write!(s, "<td class=\"text\" lang=\"{}\">{}</td>", escape_html(meta.target_lang), escape_html(&r.translated));
    if meta.back_checked {
        match &r.back {
            Some(b) => {
                let _ = write!(s, "<td class=\"text\">{}</td>", escape_html(b));
            }
            None => s.push_str("<td class=\"ok\">not checked</td>"),
        }
    }
    s.push_str("<td>");
    if r.flags.is_empty() {
        s.push_str("<span class=\"ok\">No automatic notes.</span>");
    } else {
        s.push_str("<ul class=\"checks\">");
        for f in &r.flags {
            let _ = write!(s, "<li>{}</li>", escape_html(&f.describe()));
        }
        s.push_str("</ul>");
    }
    if let Some(sim) = r.similarity {
        let _ = write!(s, "<div class=\"ok\">Back-translation match {:.0}%</div>", sim * 100.0);
    }
    s.push_str("</td></tr>\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(back: bool) -> Meta<'static> {
        Meta { document: "a & b.pdf", source_lang: "en", target_lang: "si", engine: "Google Cloud Translation", back_checked: back }
    }

    fn row(flags: Vec<Flag>) -> Row {
        Row {
            page: 2,
            index: 3,
            source: "<b>1 < 2</b> & \"x\"".into(),
            translated: "ආයුබෝවන්".into(),
            back: Some("hello".into()),
            similarity: Some(0.5),
            flags,
        }
    }

    #[test]
    fn text_is_escaped_so_a_page_cannot_inject_markup() {
        let out = html(&meta(true), &[row(vec![])]);
        assert!(out.contains("&lt;b&gt;1 &lt; 2&lt;/b&gt; &amp; &quot;x&quot;"), "{out}");
        assert!(!out.contains("<b>1"), "{out}");
        assert!(out.contains("a &amp; b.pdf"));
        assert!(!out.to_ascii_lowercase().contains("<script"));
    }

    #[test]
    fn flagged_rows_are_marked_and_counted() {
        let out = html(&meta(true), &[row(vec![]), row(vec![Flag::Empty])]);
        assert!(out.contains("2 segments, 1 with notes to check."), "{out}");
        assert_eq!(out.matches("class=\"flagged\"").count(), 1);
        assert!(out.contains("The translation is empty."));
        assert!(out.contains("No automatic notes."));
        assert!(out.contains("2.3"), "page.index position");
    }

    #[test]
    fn the_translation_cell_names_its_language() {
        assert!(html(&meta(true), &[row(vec![])]).contains("lang=\"si\">ආයුබෝවන්"));
    }

    #[test]
    fn without_back_translation_the_column_and_a_warning_say_so() {
        let out = html(&meta(false), &[row(vec![])]);
        assert!(!out.contains("Translated back"));
        assert!(out.contains("Back-translation was off"));
    }

    #[test]
    fn an_empty_run_is_still_a_valid_page() {
        let out = html(&meta(true), &[]);
        assert!(out.contains("0 segments, 0 with notes to check."));
        assert!(out.ends_with("</html>\n"));
    }

    #[test]
    fn the_translation_is_also_laid_out_alone_page_by_page_for_printing() {
        let mut first = row(vec![]);
        first.page = 1;
        let mut second = row(vec![]);
        second.translated = "දෙවන".into();
        let page = html(&meta(true), &[first, second]);
        assert!(page.contains("<h3>Page 1</h3>") && page.contains("<h3>Page 2</h3>"));
        assert!(page.contains("@media print") && page.contains(".review"));
        assert!(page.contains("<p class=\"seg\">දෙවන</p>"));
        // The review table is inside the part that printing hides.
        let table = page.find("<table>").unwrap();
        assert!(page[..table].contains("<section class=\"review\">"));
    }
}
