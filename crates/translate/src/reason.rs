//! The reason a service gives when it rejects a request ("API key not valid", "model not found").
//! The services' error replies carry no part of the request, so the sentence is safe to show; even
//! so it is cut short, and dropped if it holds a long run of key-like characters.

use crate::review::TranslateError;

const MAX_CHARS: usize = 200;

/// The error for an HTTP failure with reply text `body`: `Rejected` when the reply holds a
/// readable reason, else plain `Http(status)`.
pub fn rejected(status: u16, body: &str) -> TranslateError {
    let reason = serde_json::from_str::<serde_json::Value>(body).ok().and_then(|v| v.get("error")?.get("message")?.as_str().map(str::to_string));
    match reason {
        Some(text) if !text.trim().is_empty() && !has_key_like_run(&text) => {
            TranslateError::Rejected { status, reason: text.trim().chars().take(MAX_CHARS).collect() }
        }
        _ => TranslateError::Http(status),
    }
}

fn has_key_like_run(text: &str) -> bool {
    let mut run = 0usize;
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
            run = run.saturating_add(1);
            if run >= 30 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_readable_reason_is_kept_and_cut_short() {
        let body = r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT"}}"#;
        assert_eq!(rejected(400, body), TranslateError::Rejected { status: 400, reason: "API key not valid. Please pass a valid API key.".into() });
        let long = format!(r#"{{"error":{{"message":"{}"}}}}"#, "word ".repeat(100));
        match rejected(400, &long) {
            TranslateError::Rejected { reason, .. } => assert!(reason.chars().count() <= MAX_CHARS),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn no_reason_or_a_key_like_one_falls_back_to_the_status() {
        assert_eq!(rejected(400, "not json"), TranslateError::Http(400));
        assert_eq!(rejected(400, r#"{"error":{}}"#), TranslateError::Http(400));
        let echo = format!(r#"{{"error":{{"message":"bad key {}"}}}}"#, "A".repeat(40));
        assert_eq!(rejected(403, &echo), TranslateError::Http(403));
    }
}
