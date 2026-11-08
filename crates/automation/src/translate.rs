//! `text_translate_review`: translate pages with Google Cloud Translation and write a review page.

use pdfcraft_translate::{Google, Meta, Options, report, review};

use super::error::{Result, ToolError, failed};
use super::json::{Args, summary};
use super::{Automation, Value, json};

/// Default for `max_chars`: the most characters one run may send, back-translation included.
const DEFAULT_MAX_CHARS: i64 = 100_000;

impl Automation {
    pub(crate) fn text_translate_review(&mut self, a: &Args) -> Result<Value> {
        if a.opt_bool("confirm_send")? != Some(true) {
            return Err(ToolError::InvalidArgs(
                "This tool sends the page text to Google Cloud Translation, which is billed to the key's project. Pass confirm_send: true to allow that.".into(),
            ));
        }
        let out_path = a.str("path")?;
        let opts = translate_options(a)?;
        let mut google = Google::from_env().map_err(failed)?;
        let doc = self.doc_to_copy(a)?;
        let id = doc.id;
        let pages = match a.opt_ints("pages")? {
            Some(_) => self.pages(a, "pages")?,
            None => (0..doc.info.pages.len()).collect(),
        };
        let texts = self.page_texts(id, &pages)?;
        let page_text: Vec<(usize, String)> = pages.iter().zip(&texts).map(|(p, t)| (p.saturating_add(1), t.plain_text())).collect();
        let name = summary(self.doc(a)?)["name"].as_str().unwrap_or("document").to_owned();
        let done = review(&page_text, &mut google, &opts).map_err(failed)?;
        let flagged = done.rows.iter().filter(|r| !r.flags.is_empty()).count();
        let source = opts.source.as_deref().or(done.detected_source.as_deref()).unwrap_or("auto");
        let meta = Meta {
            document: &name,
            source_lang: source,
            target_lang: &opts.target,
            engine: "Google Cloud Translation",
            back_checked: opts.back_check,
        };
        self.write_output(out_path, report::html(&meta, &done.rows).as_bytes())?;
        Ok(json!({
            "path": out_path,
            "segments": done.rows.len(),
            "flagged": flagged,
            "characters_sent": done.chars_sent,
            "source": source,
            "target": opts.target,
            "back_checked": opts.back_check,
        }))
    }
}

fn translate_options(a: &Args) -> Result<Options> {
    let mut opts = Options::new(a.opt_str("target")?.unwrap_or("si"));
    opts.source = a.opt_str("source")?.map(str::to_owned);
    opts.back_check = a.opt_bool("back_check")?.unwrap_or(true);
    let max = a.opt_int("max_chars")?.unwrap_or(DEFAULT_MAX_CHARS);
    opts.max_total_chars = usize::try_from(max).map_err(|_| ToolError::InvalidArgs("max_chars must be positive".into()))?;
    if let Some(s) = a.opt_num("min_similarity")? {
        if !(0.0..=1.0).contains(&s) {
            return Err(ToolError::InvalidArgs("min_similarity must be between 0 and 1".into()));
        }
        opts.min_similarity = s;
    }
    Ok(opts)
}
