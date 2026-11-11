//! Export a PDF ▸ Image (PNG, JPEG, TIFF) and Text (Acrobat's Export a PDF tool, first formats).
//!
//! On the desktop the export runs on a worker thread and reports progress in the notice bar;
//! on the web it runs in place and downloads the files.

use std::sync::{Arc, Mutex};

use egui::{Align, Layout};
use pdfcraft_engine::export::{ExportSource, Exporter, ImageFormat};

use crate::marks_ui::PageRange;
use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, widgets};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportKind {
    Image,
    Text,
    /// Export all images: the images pages use, as files.
    AllImages,
    /// Translate the text and write a review page (a Google or Gemini key from the environment).
    Translate,
}

/// The service that translates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TranslateEngine {
    Google,
    Gemini,
}

impl TranslateEngine {
    fn label(self) -> &'static str {
        match self {
            TranslateEngine::Google => "Google Cloud Translation",
            TranslateEngine::Gemini => "Gemini",
        }
    }

    fn other(self) -> Self {
        match self {
            TranslateEngine::Google => TranslateEngine::Gemini,
            TranslateEngine::Gemini => TranslateEngine::Google,
        }
    }

    /// A client for this service with `key`, or with the key from the environment when `key` is empty.
    fn client(self, key: &str) -> Result<Box<dyn pdfcraft_translate::Translator>, pdfcraft_translate::TranslateError> {
        use pdfcraft_translate::{Gemini, Google, gemini::DEFAULT_MODEL};
        let key = pdfcraft_translate::keys::tidy_key(key);
        let key = key.as_str();
        Ok(match (self, key.is_empty()) {
            (TranslateEngine::Google, true) => Box::new(Google::from_env()?),
            (TranslateEngine::Google, false) => Box::new(Google::new(key)?),
            (TranslateEngine::Gemini, true) => Box::new(Gemini::from_env()?),
            (TranslateEngine::Gemini, false) => Box::new(Gemini::new(key, DEFAULT_MODEL)?),
        })
    }
}

/// The Translate dialog's choices.
#[derive(Clone, PartialEq)]
pub struct TranslateDraft {
    /// Target language code, such as `si`.
    pub target: String,
    pub engine: TranslateEngine,
    /// Translate back and compare, to flag doubtful passages.
    pub back_check: bool,
    /// Let the other service make the translation back.
    pub check_with_other: bool,
    /// The person agreed that the text is sent to the service.
    pub confirmed: bool,
    /// The API key typed in the dialog. Empty means: read it from the environment (desktop). It is
    /// used for one run, never saved, and never printed.
    pub key: String,
}

impl std::fmt::Debug for TranslateDraft {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TranslateDraft").field("target", &self.target).field("engine", &self.engine).finish_non_exhaustive()
    }
}

impl Default for TranslateDraft {
    fn default() -> Self {
        Self { target: "si".into(), engine: TranslateEngine::Gemini, back_check: true, check_with_other: false, confirmed: false, key: String::new() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportDraft {
    pub dpi: f64,
    pub range: PageRange,
    pub format: ImageFormat,
    /// Export all images ▸ skip images with fewer pixels than this on their shorter side.
    pub min_side: u32,
    pub translate: TranslateDraft,
}

impl Default for ExportDraft {
    fn default() -> Self {
        Self { dpi: 150.0, range: PageRange::default(), format: ImageFormat::Png, min_side: 0, translate: TranslateDraft::default() }
    }
}

/// Progress of a background export: (done, total, final message once finished).
pub type ExportStatus = Arc<Mutex<Option<(usize, usize, Option<String>)>>>;

pub(crate) fn body(ui: &mut egui::Ui, app: &mut PdfCraftApp, t: &Tokens, kind: ExportKind) -> (bool, bool) {
    let count = app.active_ids().and_then(|(_, id)| app.session.get(id)).map(|d| d.info.pages.len()).unwrap_or(0);
    let d = &mut app.export_draft;
    ui.label(
        egui::RichText::new(match kind {
            ExportKind::Image => tl!("Export to Image"),
            ExportKind::Text => tl!("Export to Text"),
            ExportKind::AllImages => tl!("Export All Images"),
            ExportKind::Translate => tl!("Translate for Review"),
        })
        .font(theme::semibold(18.0)),
    );
    ui.add_space(8.0);
    if kind == ExportKind::Image {
        ui.horizontal(|ui| {
            ui.label(tl!("Resolution"));
            craft_ui::ComboBox::from_id_salt("export-dpi")
                .selected_text(crate::i18n::fmt(tl!("{dpi} pixels/inch"), &[("dpi", &d.dpi.to_string())]))
                .show_ui(ui, |ui| {
                    for dpi in [72.0, 96.0, 150.0, 300.0, 600.0] {
                        ui.selectable_value(&mut d.dpi, dpi, crate::i18n::fmt(tl!("{dpi} pixels/inch"), &[("dpi", &dpi.to_string())]));
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label(tl!("Format"));
            let mut quality = match d.format {
                ImageFormat::Jpeg { quality } => quality,
                _ => 85,
            };
            craft_ui::ComboBox::from_id_salt("export-format").selected_text(d.format.label()).show_ui(ui, |ui| {
                for f in [ImageFormat::Png, ImageFormat::Jpeg { quality }, ImageFormat::Tiff] {
                    let on = std::mem::discriminant(&d.format) == std::mem::discriminant(&f);
                    if ui.selectable_label(on, f.label()).clicked() {
                        d.format = f;
                    }
                }
            });
            if let ImageFormat::Jpeg { .. } = d.format {
                ui.label(tl!("Quality"));
                if ui.add(egui::Slider::new(&mut quality, 10..=100)).changed() {
                    d.format = ImageFormat::Jpeg { quality };
                }
            }
        });
        ui.label(
            egui::RichText::new(crate::i18n::fmt(tl!("One {f} file per page, named after the document."), &[("f", d.format.label())]))
                .small()
                .color(t.text_faint),
        );
    } else if kind == ExportKind::AllImages {
        ui.horizontal(|ui| {
            ui.label(tl!("Exclude images smaller than"));
            let label = move |n: u32| {
                if n == 0 { tl!("No limit").to_string() } else { crate::i18n::trn(crate::i18n::current(), u64::from(n), "{n} pixel", "{n} pixels") }
            };
            craft_ui::ComboBox::from_id_salt("export-min").selected_text(label(d.min_side)).show_ui(ui, |ui| {
                for n in [0, 16, 32, 64, 128, 256] {
                    ui.selectable_value(&mut d.min_side, n, label(n));
                }
            });
        });
        ui.label(
            egui::RichText::new(tl!("Each image once, named after the document and page. JPEG images are saved unchanged; others as PNG."))
                .small()
                .color(t.text_faint),
        );
    } else if kind == ExportKind::Translate {
        let tr = &mut d.translate;
        ui.label(
            egui::RichText::new(tl!(
                "Translates the text and writes a review page (HTML) that flags numbers, links and passages worth checking. No translation is guaranteed to be correct: have a person check what matters."
            ))
            .small()
            .color(t.text_faint),
        );
        ui.horizontal(|ui| {
            ui.label(tl!("Translate into (language code)"));
            ui.add(egui::TextEdit::singleline(&mut tr.target).desired_width(70.0));
        });
        ui.horizontal(|ui| {
            ui.label(tl!("Service"));
            for e in [TranslateEngine::Google, TranslateEngine::Gemini] {
                widgets::radio_value(ui, &mut tr.engine, e, e.label());
            }
        });
        ui.horizontal(|ui| {
            ui.label(tl!("API key"));
            ui.add(egui::TextEdit::singleline(&mut tr.key).password(true).desired_width(260.0));
        });
        ui.label(
            egui::RichText::new(tl!(
                "The key is used for this run only and is never saved. Where the app can read the environment, an empty box uses the PDFCRAFT_GOOGLE_TRANSLATE_API_KEY or PDFCRAFT_GEMINI_API_KEY variable."
            ))
            .small()
            .color(t.text_faint),
        );
        ui.add(widgets::checkbox(&mut tr.back_check, tl!("Translate back and flag passages that differ")));
        // The second service's key can only come from the environment, which a browser has not.
        #[cfg(not(target_arch = "wasm32"))]
        ui.add_enabled(tr.back_check, widgets::checkbox(&mut tr.check_with_other, tl!("Use the other service for the check")));
        ui.add(widgets::checkbox(&mut tr.confirmed, tl!("I agree to send the text of these pages to the service")));
    } else {
        ui.label(egui::RichText::new(tl!("Plain text in reading order; pages are separated by form feeds.")).small().color(t.text_faint));
    }
    ui.add_space(6.0);
    ui.label(egui::RichText::new(tl!("Pages")).font(theme::semibold(12.5)));
    d.range.ui(ui, count);
    ui.add_space(12.0);
    let (mut apply, mut cancel) = (false, false);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let mut ok = !d.range.pages(count).is_empty();
        if kind == ExportKind::Translate {
            ok = ok && d.translate.confirmed && pdfcraft_translate::valid_lang(&d.translate.target);
            // A browser has no environment to read a key from.
            if cfg!(target_arch = "wasm32") {
                ok = ok && !d.translate.key.trim().is_empty();
            }
        }
        let label = if kind == ExportKind::Translate { tl!("Translate") } else { tl!("Export") };
        if ui.add_enabled_ui(ok, |ui| widgets::pill_button(ui, label, true)).inner.clicked() {
            apply = true;
        }
        if widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
            cancel = true;
        }
    });
    (apply, cancel)
}

/// Write the files (`name`, bytes) produced for `pages`, reporting progress.
struct ExportTask<'a> {
    src: ExportSource,
    kind: ExportKind,
    dpi: f64,
    format: ImageFormat,
    min_side: u32,
    pages: Vec<usize>,
    stem: String,
    status: &'a ExportStatus,
    lang: crate::i18n::Lang,
    translate: TranslateDraft,
}

fn run(task: ExportTask<'_>, mut sink: impl FnMut(&str, Vec<u8>) -> Result<(), String>) -> String {
    let ExportTask { src, kind, dpi, format, min_side, pages, stem, status, lang, translate } = task;
    // May run on a worker thread: messages are drawn in the UI's language.
    crate::i18n::set_current(lang);
    let total = pages.len();
    let set = |done: usize, msg: Option<String>| {
        if let Ok(mut s) = status.lock() {
            *s = Some((done, total, msg));
        }
    };
    if kind == ExportKind::AllImages {
        set(0, None);
        let out = match pdfcraft_engine::export::extract_images(&src, &pages, min_side) {
            Ok(o) => o,
            Err(e) => return crate::i18n::fmt(tl!("Export stopped: {e}"), &[("e", &e.to_string())]),
        };
        for (k, img) in out.images.iter().enumerate() {
            if let Err(e) = sink(&pdfcraft_engine::export::image_file_name(&stem, img, k + 1, src.pages), img.data.clone()) {
                return crate::i18n::fmt(tl!("Export stopped: {e}"), &[("e", &e.to_string())]);
            }
        }
        let n = out.images.len();
        let mut msg = if n == 1 {
            crate::i18n::fmt(tl!("Exported 1 image"), &[])
        } else {
            crate::i18n::fmt(tl!("Exported {n} images"), &[("n", &n.to_string())])
        };
        if !out.skipped.is_empty() {
            msg.push_str(&crate::i18n::fmt(
                tl!(" ({s} not exported: {first})"),
                &[("s", &out.skipped.len().to_string()), ("first", &out.skipped[0].2)],
            ));
        }
        return msg;
    }
    let mut ex = Exporter::from_source(src);
    match kind {
        // Handled above.
        ExportKind::AllImages => String::new(),
        ExportKind::Translate => {
            set(0, None);
            match translate_pages(&mut ex, &pages, &stem, &translate, &mut sink) {
                Ok(msg) => msg,
                Err(e) => crate::i18n::fmt(tl!("Translation stopped: {e}"), &[("e", &e)]),
            }
        }
        ExportKind::Image => {
            set(0, None);
            // Pages are drawn several at once and handed over in order: the progress counts the
            // pages written so far.
            let mut done = 0;
            let result = ex.images(&pages, dpi, format, |p, img| {
                sink(&format!("{stem}_page_{}.{}", p + 1, format.extension()), img)?;
                done += 1;
                set(done, None);
                Ok(())
            });
            if let Err(e) = result {
                return crate::i18n::fmt(tl!("Export stopped: {e}"), &[("e", &e.to_string())]);
            }
            let mut msg = if total == 1 {
                crate::i18n::fmt(tl!("Exported 1 image"), &[])
            } else {
                crate::i18n::fmt(tl!("Exported {n} images"), &[("n", &total.to_string())])
            };
            // Pages too large for the renderer at the chosen resolution are drawn at the most it
            // allows: say so, rather than leave a smaller image unexplained.
            let lowered: Vec<f64> = pages.iter().map(|p| ex.dpi_used(*p, dpi)).filter(|used| *used < dpi.clamp(18.0, 1200.0) - 0.5).collect();
            if let Some(lowest) = lowered.iter().copied().reduce(f64::min) {
                msg.push_str(&crate::i18n::fmt(
                    tl!(" ({n} at {dpi} dpi, the largest size they can be drawn at)"),
                    &[("n", &lowered.len().to_string()), ("dpi", &format!("{}", lowest.floor()))],
                ));
            }
            msg
        }
        ExportKind::Text => {
            set(0, None);
            match ex.text_of(&pages).and_then(|text| sink(&format!("{stem}.txt"), text.into_bytes())) {
                Ok(()) => {
                    if total == 1 {
                        crate::i18n::fmt(tl!("Exported the text of 1 page"), &[])
                    } else {
                        crate::i18n::fmt(tl!("Exported the text of {n} pages"), &[("n", &total.to_string())])
                    }
                }
                Err(e) => crate::i18n::fmt(tl!("Export stopped: {e}"), &[("e", &e.to_string())]),
            }
        }
    }
}

/// The text of `pages` with 1-based page numbers.
fn collect_text(ex: &mut Exporter, pages: &[usize]) -> Result<Vec<(usize, String)>, String> {
    let mut page_text = Vec::with_capacity(pages.len());
    for p in pages {
        page_text.push((p.saturating_add(1), ex.text(*p)?));
    }
    Ok(page_text)
}

/// Translate `page_text` and check it. Gives a sentence for the status line and the review page.
/// The text is sent only after the dialog's consent box was ticked (the callers check).
fn translate_text(page_text: &[(usize, String)], stem: &str, d: &TranslateDraft) -> Result<(String, Vec<u8>), pdfcraft_translate::TranslateError> {
    use pdfcraft_translate::{Meta, Options, report, review, review_with_checker};
    let mut forward = d.engine.client(&d.key)?;
    let mut checker = if d.back_check && d.check_with_other { Some(d.engine.other().client("")?) } else { None };
    let mut opts = Options::new(&d.target);
    opts.back_check = d.back_check;
    let done = match checker.as_mut() {
        Some(c) => review_with_checker(page_text, forward.as_mut(), c.as_mut(), &opts),
        None => review(page_text, forward.as_mut(), &opts),
    }?;
    let flagged = done.rows.iter().filter(|r| !r.flags.is_empty()).count();
    let source = done.detected_source.as_deref().unwrap_or("auto");
    let engine = match checker {
        Some(_) => format!("{} (checked with {})", d.engine.label(), d.engine.other().label()),
        None => d.engine.label().to_owned(),
    };
    let meta = Meta { document: stem, source_lang: source, target_lang: &opts.target, engine: &engine, back_checked: d.back_check };
    let html = report::html(&meta, &done.rows).into_bytes();
    let msg = crate::i18n::fmt(
        tl!("Translated {n} passages; {f} need a look in the review page"),
        &[("n", &done.rows.len().to_string()), ("f", &flagged.to_string())],
    );
    Ok((msg, html))
}

/// Translate the text of `pages`, check it, and hand the review page to `sink`.
fn translate_pages(
    ex: &mut Exporter,
    pages: &[usize],
    stem: &str,
    d: &TranslateDraft,
    sink: &mut impl FnMut(&str, Vec<u8>) -> Result<(), String>,
) -> Result<String, String> {
    if !d.confirmed {
        return Err(tl!("the text is sent to a service only after you agree").to_string());
    }
    let page_text = collect_text(ex, pages)?;
    let (msg, html) = translate_text(&page_text, stem, d).map_err(|e| e.to_string())?;
    sink(&format!("{stem}_translation_review.html"), html)?;
    Ok(msg)
}

/// One request from the review, sent with the browser's `fetch`. Only the status or a fixed error
/// leaves it: the reply text and the URL (which holds a Google key) are not kept in errors.
#[cfg(target_arch = "wasm32")]
async fn browser_fetch(p: pdfcraft_translate::replay::Pending) -> Result<String, pdfcraft_translate::TranslateError> {
    use pdfcraft_translate::TranslateError::{Http, Rejected};
    // A busy service (503) or a rate limit (429) often clears within seconds: wait and ask again.
    let mut result = fetch_once(&p).await;
    for wait_ms in [3_000, 8_000] {
        if !matches!(result, Err(Http(503 | 429) | Rejected { status: 503 | 429, .. })) {
            break;
        }
        sleep_ms(wait_ms).await;
        result = fetch_once(&p).await;
    }
    result
}

/// Wait `ms` milliseconds without blocking the page.
#[cfg(target_arch = "wasm32")]
async fn sleep_ms(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        if let Some(window) = web_sys::window() {
            // Only fails for a bad callback; then there is simply no wait.
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        } else {
            let _ = resolve.call0(&wasm_bindgen::JsValue::NULL);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

#[cfg(target_arch = "wasm32")]
async fn fetch_once(p: &pdfcraft_translate::replay::Pending) -> Result<String, pdfcraft_translate::TranslateError> {
    use pdfcraft_translate::TranslateError::Network;
    use wasm_bindgen::JsCast;
    let window = web_sys::window().ok_or(Network)?;
    let headers = web_sys::Headers::new().map_err(|_| Network)?;
    headers.set("Content-Type", "application/json").map_err(|_| Network)?;
    if let Some((name, value)) = &p.header {
        headers.set(name, value).map_err(|_| Network)?;
    }
    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    init.set_headers(&headers);
    init.set_body(&wasm_bindgen::JsValue::from_str(&p.body));
    init.set_mode(web_sys::RequestMode::Cors);
    let request = web_sys::Request::new_with_str_and_init(&p.url, &init).map_err(|_| Network)?;
    let response = wasm_bindgen_futures::JsFuture::from(window.fetch_with_request(&request)).await.map_err(|_| Network)?;
    let response: web_sys::Response = response.dyn_into().map_err(|_| Network)?;
    let text = wasm_bindgen_futures::JsFuture::from(response.text().map_err(|_| Network)?).await.map_err(|_| Network)?;
    if !response.ok() {
        return Err(pdfcraft_translate::reason::rejected(response.status(), &text.as_string().unwrap_or_default()));
    }
    text.as_string().ok_or(Network)
}

impl PdfCraftApp {
    /// Start exporting the active document with the dialog's settings.
    pub(crate) fn start_export(&mut self, kind: ExportKind) {
        // What's typed in a form field is part of the document (#166).
        if !self.commit_form_typing() {
            return;
        }
        let Some((_, id)) = self.active_ids() else { return };
        if !self.may_copy(id) {
            return;
        }
        let Some(doc) = self.session.get(id) else { return };
        let src = doc.export_source();
        let stem = doc.name.trim_end_matches(".pdf").trim_end_matches(".PDF").to_string();
        let pages = self.export_draft.range.pages(src.pages);
        let dpi = self.export_draft.dpi;
        let format = self.export_draft.format;
        let min_side = self.export_draft.min_side;
        let translate = self.export_draft.translate.clone();
        let status: ExportStatus = Arc::new(Mutex::new(Some((0, pages.len(), None))));
        #[cfg(not(target_arch = "wasm32"))]
        {
            let start = move |app: &mut Self, dir: std::path::PathBuf, inline: bool| {
                let st = status.clone();
                let shown = dir.display().to_string();
                let lang = crate::i18n::current();
                let work = move || {
                    let sink = |name: &str, bytes: Vec<u8>| {
                        crate::editing::write_atomically(&dir.join(name).to_string_lossy(), &bytes).map_err(|e| format!("{name}: {e}"))
                    };
                    let msg = run(ExportTask { src, kind, dpi, format, min_side, pages, stem, status: &st, lang, translate }, sink);
                    if let Ok(mut s) = st.lock() {
                        let (done, total) = s.as_ref().map_or((0, 0), |(d, t, _)| (*d, *t));
                        let done_msg = crate::i18n::fmt(tl!("{msg} to {dir}"), &[("msg", &msg), ("dir", &shown)]);
                        *s = Some((done.max(total), total, Some(done_msg)));
                    }
                };
                if inline {
                    work(); // tests and automation: synchronous
                } else {
                    std::thread::Builder::new().name("pdfcraft-export".into()).spawn(work).ok();
                }
                app.export_status = Some(status);
            };
            match self.export_dir_override.clone() {
                Some(d) => start(self, d.into(), true),
                None => {
                    let dialog = rfd::AsyncFileDialog::new().set_title(tl!("Choose a folder for the exported files").to_string());
                    self.ask_one(crate::pickers::Ask::Folder(dialog), None, move |app, dir| start(app, dir, false));
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            if kind == ExportKind::Translate {
                self.start_translate_in_browser(src, pages, stem, translate);
                return;
            }
            let msg = run(
                ExportTask { src, kind, dpi, format, min_side, pages, stem, status: &status, lang: crate::i18n::current(), translate },
                |name, bytes| crate::editing::download(name, &bytes),
            );
            if let Ok(mut s) = status.lock() {
                *s = Some((0, 0, Some(msg)));
            }
            self.export_status = Some(status);
        }
    }

    /// Translate in the browser: the requests go out with `fetch` one after another while the page
    /// stays usable, then the review page is offered as a download.
    #[cfg(target_arch = "wasm32")]
    fn start_translate_in_browser(&mut self, src: pdfcraft_engine::export::ExportSource, pages: Vec<usize>, stem: String, d: TranslateDraft) {
        if !d.confirmed {
            self.notify_tr("The text is sent to a service only after you agree");
            return;
        }
        let mut ex = Exporter::from_source(src);
        let page_text = match collect_text(&mut ex, &pages) {
            Ok(t) => t,
            Err(e) => {
                self.notify(crate::i18n::fmt(tl!("Translation stopped: {e}"), &[("e", &e)]));
                return;
            }
        };
        let status: ExportStatus = Arc::new(Mutex::new(Some((0, 0, None))));
        let st = status.clone();
        let lang = crate::i18n::current();
        wasm_bindgen_futures::spawn_local(async move {
            crate::i18n::set_current(lang);
            let result = pdfcraft_translate::replay::drive(|| translate_text(&page_text, &stem, &d), browser_fetch).await;
            let msg = match result {
                Ok((msg, html)) => match crate::editing::download_as(&format!("{stem}_translation_review.html"), &html, "text/html") {
                    Ok(()) => msg,
                    Err(e) => crate::i18n::fmt(tl!("Translation stopped: {e}"), &[("e", &e)]),
                },
                Err(e) => crate::i18n::fmt(tl!("Translation stopped: {e}"), &[("e", &e.to_string())]),
            };
            if let Ok(mut s) = st.lock() {
                *s = Some((0, 0, Some(msg)));
            }
        });
        self.export_status = Some(status);
        self.notify_tr("Translating… this can take a minute");
    }

    /// Show export progress, and the result once it is done.
    pub(crate) fn poll_export(&mut self) {
        let Some(st) = self.export_status.clone() else { return };
        let snapshot = st.lock().ok().and_then(|s| s.clone());
        match snapshot {
            Some((_, _, Some(msg))) => {
                self.export_status = None;
                self.notify(msg);
            }
            Some((done, total, None)) if total > 1 => {
                let m = crate::i18n::fmt(tl!("Exporting… {d} of {t}"), &[("d", &done.to_string()), ("t", &total.to_string())]);
                self.notify(m);
            }
            _ => {}
        }
        if let Some(ctx) = &self.ctx {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }
}
