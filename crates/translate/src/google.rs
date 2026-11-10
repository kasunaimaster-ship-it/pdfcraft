//! Google Cloud Translation (Basic, v2) over HTTPS.
//!
//! The API key comes from the environment and is never part of an error message, a log line or
//! a returned value. Only the HTTP status or a fixed sentence leaves this module on failure.

use std::time::Duration;

use crate::http;
use crate::keys::check_key;
use crate::review::{TranslateError, Translated, Translator};

/// The environment variable that holds the API key.
pub const KEY_ENV: &str = "PDFCRAFT_GOOGLE_TRANSLATE_API_KEY";

const ENDPOINT: &str = "https://translation.googleapis.com/language/translate/v2";

pub struct Google {
    key: String,
    agent: http::Agent,
}

impl Google {
    /// A client using the key in [`KEY_ENV`].
    pub fn from_env() -> Result<Self, TranslateError> {
        let key = std::env::var(KEY_ENV).map_err(|_| TranslateError::MissingKey(KEY_ENV))?;
        Self::new(key.trim())
    }

    pub fn new(key: &str) -> Result<Self, TranslateError> {
        check_key(key, KEY_ENV)?;
        Ok(Google { key: key.to_owned(), agent: http::agent(Duration::from_secs(60)) })
    }
}

impl Translator for Google {
    fn translate(&mut self, texts: &[String], source: Option<&str>, target: &str) -> Result<Vec<Translated>, TranslateError> {
        let payload = request_body(texts, source, target);
        let url = format!("{ENDPOINT}?key={}", self.key);
        parse_response(&http::post_json(&self.agent, &url, None, &payload)?)
    }
}

/// The JSON request. `format` is `text` so the service does not treat the page text as HTML.
fn request_body(texts: &[String], source: Option<&str>, target: &str) -> String {
    let mut body = serde_json::json!({ "q": texts, "target": target, "format": "text" });
    if let (Some(s), Some(obj)) = (source, body.as_object_mut()) {
        obj.insert("source".into(), serde_json::Value::String(s.to_owned()));
    }
    body.to_string()
}

fn bad(msg: &str) -> TranslateError {
    TranslateError::BadResponse(msg.to_owned())
}

fn parse_response(text: &str) -> Result<Vec<Translated>, TranslateError> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|_| bad("the reply is not JSON"))?;
    let list =
        value.get("data").and_then(|d| d.get("translations")).and_then(|t| t.as_array()).ok_or_else(|| bad("the reply has no translations"))?;
    list.iter()
        .map(|item| {
            let text = item.get("translatedText").and_then(|t| t.as_str()).ok_or_else(|| bad("a translation has no text"))?;
            let detected = item.get("detectedSourceLanguage").and_then(|d| d.as_str()).map(str::to_owned);
            Ok(Translated { text: text.to_owned(), detected })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_asks_for_plain_text_and_names_the_languages() {
        let v: serde_json::Value = serde_json::from_str(&request_body(&["a".into(), "b".into()], Some("en"), "si")).unwrap();
        assert_eq!(v["q"], serde_json::json!(["a", "b"]));
        assert_eq!(v["target"], "si");
        assert_eq!(v["source"], "en");
        assert_eq!(v["format"], "text");
        let auto: serde_json::Value = serde_json::from_str(&request_body(&["a".into()], None, "si")).unwrap();
        assert!(auto.get("source").is_none(), "no source means the service detects it");
    }

    #[test]
    fn a_reply_is_read_with_and_without_detected_languages() {
        let got = parse_response(r#"{"data":{"translations":[{"translatedText":"ආයුබෝවන්","detectedSourceLanguage":"en"},{"translatedText":"x"}]}}"#)
            .unwrap();
        assert_eq!(got[0], Translated { text: "ආයුබෝවන්".into(), detected: Some("en".into()) });
        assert_eq!(got[1], Translated { text: "x".into(), detected: None });
    }

    #[test]
    fn a_reply_that_is_not_a_translation_is_an_error_not_a_panic() {
        for bad in ["", "not json", "{}", r#"{"data":{}}"#, r#"{"data":{"translations":[{}]}}"#, r#"{"data":{"translations":"x"}}"#] {
            assert!(matches!(parse_response(bad), Err(TranslateError::BadResponse(_))), "{bad}");
        }
    }

    #[test]
    fn the_client_refuses_keys_that_could_change_the_url() {
        assert_eq!(Google::new("").err(), Some(TranslateError::MissingKey(KEY_ENV)));
        assert!(matches!(Google::new("a&b=c").err(), Some(TranslateError::BadKeyChar { .. })));
        assert!(Google::new("test-key_0123456789").is_ok());
    }

    #[test]
    fn errors_never_contain_the_key() {
        let key = "NOT-A-REAL-KEY-123";
        for e in [
            TranslateError::Network,
            TranslateError::Http(403),
            TranslateError::BadResponse("x".into()),
            TranslateError::BadKey,
            TranslateError::BadKeyChar { position: 3, found: '&' },
            TranslateError::MissingKey(KEY_ENV),
        ] {
            assert!(!e.to_string().contains(key));
        }
    }
}
