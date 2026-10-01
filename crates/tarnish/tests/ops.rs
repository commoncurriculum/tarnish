//! Transform ops, and `textBetween` and `textContent`, as the real ProseMirror packages ran
//! them, recorded by `harness/record-ops.mjs`: tarnish must give the same documents, steps,
//! text and errors.

use tarnish::json::{Value, json};
use tarnish::{Node, Result, Schema, Text, api};
use tarnish_fixtures::{expect, outcome, read, schemas};

fn fixtures() -> (Vec<Schema>, Value) {
    let fixtures = read("ops");
    (schemas(&fixtures), fixtures)
}

fn doc(schemas: &[Schema], case: &Value) -> Node<'static> {
    let schema = &schemas[case["schema"].as_u64().expect("a schema's index") as usize];
    Node::from_json(schema, &case["doc"]).expect("a document")
}

/// Text with a lone surrogate is recorded as its JSON.
fn expect_text(case: &Value, text: Result<Text>) {
    let text = text.map(|text| match case.get("resultJSON") {
        Some(_) => json!({ "resultJSON": text.to_json_string() }),
        None => json!({ "result": text.as_str() }),
    });
    expect(case, outcome(text));
}

#[test]
fn ops_give_prosemirrors_document_steps_and_errors() {
    let (schemas, fixtures) = fixtures();
    for case in fixtures["transforms"].as_array().expect("transforms") {
        let doc = doc(&schemas, case);
        let before = doc.to_json();
        let transformed = api::transform(&doc, &case["ops"]);
        expect(
            case,
            outcome(
                transformed
                    .map(|(changed, steps)| json!({ "result": changed.to_json(), "steps": steps })),
            ),
        );
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
