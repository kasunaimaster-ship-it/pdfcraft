//! Translation review for PdfCraft: page text goes to a translation service the user chose and
//! the result comes back as a table a person can check, with automatic flags on the rows most
//! likely to be wrong.
//!
//! Machine translation cannot be proven correct. The checks here find a few kinds of problem
//! (numbers and links that changed, text left untranslated, a back-translation that no longer
//! means the same). A row with no flag is not a verified row.
//!
//! The text is sent over the network only when the caller asks for it: nothing in this crate
//! runs by default, and the API key is read from the environment, never stored or logged.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod check;
pub mod google;
pub mod report;
pub mod review;
pub mod segment;

pub use check::{CheckOptions, Flag};
pub use google::{Google, KEY_ENV};
pub use report::{Meta, Row, escape_html, html};
pub use review::{Options, Review, TranslateError, Translated, Translator, review, valid_lang};
