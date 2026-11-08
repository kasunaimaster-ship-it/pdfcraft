//! `text_translate_review`: translate pages with Google Cloud Translation or Gemini and write a
//! review page.

use pdfcraft_translate::{Gemini, Google, Meta, Options, Translator, report, review, review_with_checker};

use super::error::{Result, ToolError, failed};
use super::json::{Args, summary};
use super::{Automation, Value, json};

/// Default for `max_chars`: the most characters one run may send, back-translation included.
const DEFAULT_MAX_CHARS: i64 = 100_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Engine {
    Google,
    Gemini,
}

impl Engine {
    fn parse(name: &str) -> Result<Self> {
        match name {
            "google" => Ok(Engine::Google),
            "gemini" => Ok(Engine::Gemini),
            other => Err(ToolError::InvalidArgs(format!("engine must be \"google\" or \"gemini\", not {other:?}"))),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Engine::Google => "Google Cloud Translation",
            Engine::Gemini => "Gemini",
        }
    }

    /// A client for this service. The key comes from the environment; a missing or malformed key
    /// is an error here, before any document is read.
    fn client(self) -> Result<Box<dyn Translator>> {
        Ok(match self {
            Engine::Google => Box::new(Google::from_env().map_err(failed)?),
            Engine::Gemini => Box::new(Gemini::from_env().map_err(failed)?),
        })
    }
}

impl Automation {
    pub(crate) fn text_translate_review(&mut self, a: &Args) -> Result<Value> {
        if a.opt_bool("confirm_send")? != Some(true) {
            return Err(ToolError::InvalidArgs(
                "This tool sends the page text to the service you chose (Google Cloud Translation or Gemini). Google Cloud Translation is billed to the key's project; on Gemini's free tier Google may use the text to improve its products (see Google's Gemini API terms). Pass confirm_send: true to allow that."
                    .into(),
            ));
        }
        let out_path = a.str("path")?;
        let opts = translate_options(a)?;
        let engine = Engine::parse(a.opt_str("engine")?.unwrap_or("google"))?;
        let back_engine = match a.opt_str("back_engine")? {
            Some(name) => Engine::parse(name)?,
            None => engine,
        };
        let mut forward = engine.client()?;
        let mut checker = if opts.back_check && back_engine != engine { Some(back_engine.client()?) } else { None };
        let doc = self.doc_to_copy(a)?;
        let id = doc.id;
        let pages = match a.opt_ints("pages")? {
            Some(_) => self.pages(a, "pages")?,
            None => (0..doc.info.pages.len()).collect(),
        };
        let texts = self.page_texts(id, &pages)?;
        let page_text: Vec<(usize, String)> = pages.iter().zip(&texts).map(|(p, t)| (p.saturating_add(1), t.plain_text())).collect();
        let name = summary(self.doc(a)?)["name"].as_str().unwrap_or("document").to_owned();
        let done = match checker.as_mut() {
            Some(c) => review_with_checker(&page_text, forward.as_mut(), c.as_mut(), &opts),
            None => review(&page_text, forward.as_mut(), &opts),
        }
        .map_err(failed)?;
        let flagged = done.rows.iter().filter(|r| !r.flags.is_empty()).count();
        let source = opts.source.as_deref().or(done.detected_source.as_deref()).unwrap_or("auto");
        let engine_label = match checker {
            Some(_) => format!("{} (translated back with {})", engine.label(), back_engine.label()),
            None => engine.label().to_owned(),
        };
        let meta = Meta { document: &name, source_lang: source, target_lang: &opts.target, engine: &engine_label, back_checked: opts.back_check };
        self.write_output(out_path, report::html(&meta, &done.rows).as_bytes())?;
        Ok(json!({
            "path": out_path,
            "segments": done.rows.len(),
            "flagged": flagged,
            "characters_sent": done.chars_sent,
            "source": source,
            "target": opts.target,
            "engine": engine.label(),
            "back_engine": if opts.back_check { back_engine.label() } else { "none" },
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
