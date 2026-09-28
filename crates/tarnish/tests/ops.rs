//! Transform ops, and `textBetween` and `textContent`, as the real ProseMirror packages ran
//! them, recorded by `harness/record-ops.mjs`: tarnish must give the same documents, steps,
//! text and errors.

use tarnish::json::{self, Value};
use tarnish::{Node, Result, Schema, Text, api};

fn fixtures() -> (Vec<Schema>, Value) {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/ops.json");
    let fixtures =
        json::from_str(&std::fs::read_to_string(path).expect("the fixtures")).expect("JSON");
    let schemas = fixtures["schemas"]
        .as_array()
        .expect("schemas")
        .iter()
        .map(|spec| api::schema(spec).expect("a schema"))
        .collect();
    (schemas, fixtures)
}

fn doc(schemas: &[Schema], case: &Value) -> Node<'static> {
    let schema = &schemas[case["schema"].as_u64().expect("a schema's index") as usize];
    Node::from_json(schema, &case["doc"]).expect("a document")
}

fn expect_error(case: &Value, error: tarnish::Error) {
    let recorded = case
        .get("error")
        .unwrap_or_else(|| panic!("{error} in {case}"));
    assert_eq!(error.class(), recorded["class"], "{case}");
    assert_eq!(error.message(), recorded["message"], "{case}");
}

/// Text with a lone surrogate is recorded as its JSON.
fn expect_text(case: &Value, text: Result<Text>) {
    match (text, case.get("resultJSON")) {
        (Ok(text), Some(json)) => assert_eq!(json, &text.to_json_string(), "{case}"),
        (Ok(text), None) => assert_eq!(text.as_str(), case["result"].as_str(), "{case}"),
        (Err(error), _) => expect_error(case, error),
    }
}

#[test]
fn ops_give_prosemirrors_document_steps_and_errors() {
    let (schemas, fixtures) = fixtures();
    for case in fixtures["transforms"].as_array().expect("transforms") {
        let doc = doc(&schemas, case);
        let before = doc.to_json();
        match api::transform(&doc, &case["ops"]) {
            Ok((changed, steps)) => {
                assert!(changed.to_json() == case["result"], "{case}");
                assert!(steps == case["steps"], "{case}");
            }
            Err(error) => expect_error(case, error),
        }
        assert!(doc.to_json() == before, "{case}");
    }
}

#[test]
fn text_between_is_prosemirrors() {
    let (schemas, fixtures) = fixtures();
    for case in fixtures["textBetween"].as_array().expect("cases") {
        let position = |key: &str| case[key].as_u64().expect("a position") as usize;
        let text = api::text_between(
            &doc(&schemas, case),
            position("from"),
            position("to"),
            case.get("blockSeparator").and_then(Value::as_str),
            case.get("leafText").and_then(Value::as_str),
        );
        expect_text(case, text);
    }
}

#[test]
fn text_content_is_prosemirrors() {
    let (schemas, fixtures) = fixtures();
    for case in fixtures["textContent"].as_array().expect("cases") {
        expect_text(case, api::text_content(&doc(&schemas, case)));
    }
}
