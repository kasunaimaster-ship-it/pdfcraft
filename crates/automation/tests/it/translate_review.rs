//! `text_translate_review` sends page text to a paid web service, so it must ask first.

use pdfcraft_automation::{Automation, ToolError, tools};
use serde_json::json;

#[test]
fn translating_needs_explicit_consent_before_anything_else_happens() {
    let mut a = Automation::new();
    // Not given at all: the schema refuses the call.
    let e = a.call("text_translate_review", &json!({ "doc": 1, "path": "out.html" })).unwrap_err();
    assert!(matches!(e, ToolError::InvalidArgs(_)), "{e:?}");
    // Given as false: refused with the reason, before any document or key is looked at.
    let e = a.call("text_translate_review", &json!({ "doc": 1, "path": "out.html", "confirm_send": false })).unwrap_err();
    match e {
        ToolError::InvalidArgs(m) => assert!(m.contains("confirm_send") && m.contains("Google"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_unknown_engine_is_refused_before_any_key_is_read() {
    let mut a = Automation::new();
    let e = a.call("text_translate_review", &json!({ "doc": 1, "path": "out.html", "confirm_send": true, "engine": "deepl" })).unwrap_err();
    match e {
        ToolError::InvalidArgs(m) => assert!(m.contains("engine") || m.contains("deepl"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_tool_writes_a_file_and_never_takes_the_key_as_an_argument() {
    let t = tools().into_iter().find(|t| t.name == "text_translate_review").unwrap();
    assert!(t.destructive && !t.read_only, "it overwrites the review file");
    assert!(t.description.contains("PDFCRAFT_GOOGLE_TRANSLATE_API_KEY") && t.description.contains("PDFCRAFT_GEMINI_API_KEY"));
    let props = t.input_schema["properties"].as_object().unwrap();
    assert!(!props.keys().any(|k| k.contains("key")), "an argument would put the key in logs and transcripts: {props:?}");
    assert!(t.input_schema["required"].as_array().unwrap().iter().any(|r| r == "confirm_send"));
}
