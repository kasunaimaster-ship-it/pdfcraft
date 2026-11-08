//! Checks on values that end up in a URL or a header, and one helper for model replies.

use crate::review::TranslateError;

/// An API key is letters, digits, `-` and `_`. Anything else could change the URL or header it is
/// put in, so it is refused. `var` names the environment variable, for the error.
pub(crate) fn check_key(key: &str, var: &'static str) -> Result<(), TranslateError> {
    if key.is_empty() {
        return Err(TranslateError::MissingKey(var));
    }
    if key.len() > 200 || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(TranslateError::BadKey);
    }
    Ok(())
}

/// Whether `model` can be put in a URL path as a model name (`gemini-3.5-flash-lite`).
pub fn valid_model(model: &str) -> bool {
    (1..=60).contains(&model.len())
        && model.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        && !model.starts_with('.')
        && !model.contains("..")
}

/// The JSON inside a model reply that may be wrapped in a Markdown code fence.
pub fn unfence(text: &str) -> &str {
    let t = text.trim();
    let Some(rest) = t.strip_prefix("```") else { return t };
    let rest = rest.trim_start_matches(|c: char| c.is_ascii_alphabetic());
    rest.trim().strip_suffix("```").map(str::trim).unwrap_or(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_that_could_change_a_url_or_header_are_refused() {
        assert_eq!(check_key("", "V"), Err(TranslateError::MissingKey("V")));
        for bad in ["a&b", "a b", "a/b", "a?b=c", "කේතය", "k\n", "k\r\nHost: x"] {
            assert_eq!(check_key(bad, "V"), Err(TranslateError::BadKey), "{bad:?}");
        }
        assert_eq!(check_key(&"a".repeat(201), "V"), Err(TranslateError::BadKey));
        assert_eq!(check_key("test-key_0123456789", "V"), Ok(()));
    }

    #[test]
    fn model_names_are_checked() {
        for ok in ["gemini-3.5-flash-lite", "gemini-3.8-flash", "m_1"] {
            assert!(valid_model(ok), "{ok}");
        }
        for bad in ["", ".x", "a..b", "a/b", "a b", "a:generate", "m?key=1", &"m".repeat(61)] {
            assert!(!valid_model(bad), "{bad}");
        }
    }

    #[test]
    fn code_fences_are_taken_off_a_reply() {
        assert_eq!(unfence("[1]"), "[1]");
        assert_eq!(unfence("  [1]\n"), "[1]");
        assert_eq!(unfence("```json\n[1, 2]\n```"), "[1, 2]");
        assert_eq!(unfence("```\n[1]\n```\n"), "[1]");
        assert_eq!(unfence("```json\n[1]"), "```json\n[1]", "an unclosed fence is left alone, so parsing fails loudly");
        assert_eq!(unfence(""), "");
    }
}
