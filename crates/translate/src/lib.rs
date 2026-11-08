//! Translation review for PdfCraft: page text goes to a translation service the user chose and
//! the result comes back as a table a person can check, with automatic flags on the rows most
//! likely to be wrong.
//!
//! Machine translation cannot be proven correct. The checks here find a few kinds of problem
//! (numbers and links that changed, text left untranslated, a back-translation that no longer
//! means the same). A row with no flag is not a verified row.
//!
//! Two services are supported: Google Cloud Translation (a translation engine) and Gemini (a
//! language model, which can paraphrase and so needs the review more). They can be combined, one
//! translating and the other translating back.
//!
//! The text is sent over the network only when the caller asks for it: nothing in this crate
//! runs by default, and API keys are read from the environment, never stored or logged.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod check;
pub mod gemini;
pub mod google;
mod http;
pub mod keys;
pub mod report;
pub mod review;
pub mod segment;

pub use check::{CheckOptions, Flag};
pub use gemini::Gemini;
pub use google::Google;
pub use report::{Meta, Row, escape_html, html};
pub use review::{Options, Review, TranslateError, Translated, Translator, review, review_with_checker, valid_lang};
