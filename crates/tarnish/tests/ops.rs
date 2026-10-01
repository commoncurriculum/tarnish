//! Transform ops, and `textBetween` and `textContent`, as the real ProseMirror packages ran
//! them, recorded by `harness/record-ops.mjs`: tarnish must give the same documents, steps,
//! text and errors.

use tarnish::json::{Value, json};
use tarnish::{Node, Result, Schema, Text, api};
use tarnish_fixtures::{expect, outcome, read, records, schemas};

fn fixtures() -> (Vec<Schema>, Value) {
    let fixtures = read("ops");
    (schemas(&fixtures), fixtures)
}

fn doc(schemas: &[Schema], case: &Value) -> Node<'static> {
    let schema = &schemas[case["schema"].as_u64().expect("a schema's index") as usize];
    Node::from_json(schema, &case["doc"]).expect("a document")
}

/// Text with a lone surrogate is recorded as its JSON.
fn text_outcome(text: Result<Text>) -> Value {
    outcome(text.map(|text| match text.as_str() {
        Some(text) => json!({ "result": text }),
        None => json!({ "resultJSON": text.to_json_string() }),
    }))
}

#[test]
fn ops_give_prosemirrors_document_steps_and_errors() {
    let (schemas, fixtures) = fixtures();
    for case in records(&fixtures, "transforms") {
        let doc = doc(&schemas, case);
        let before = doc.to_json();
        let transformed = api::transform(&doc, &case["ops"]);
        let transformed = transformed
            .map(|(changed, steps)| json!({ "result": changed.to_json(), "steps": steps }));
        expect(case, &["schema", "doc", "ops"], outcome(transformed));
        assert!(doc.to_json() == before, "{case}");
    }
}

#[test]
fn text_between_is_prosemirrors() {
    let (schemas, fixtures) = fixtures();
    for case in records(&fixtures, "textBetween") {
        let position = |key: &str| case[key].as_u64().expect("a position") as usize;
        let text = api::text_between(
            &doc(&schemas, case),
            position("from"),
            position("to"),
            case.get("blockSeparator").and_then(Value::as_str),
            case.get("leafText").and_then(Value::as_str),
        );
        let inputs = ["schema", "doc", "from", "to", "blockSeparator", "leafText"];
        expect(case, &inputs, text_outcome(text));
    }
}

#[test]
fn text_content_is_prosemirrors() {
    let (schemas, fixtures) = fixtures();
    for case in records(&fixtures, "textContent") {
        let text = api::text_content(&doc(&schemas, case));
        expect(case, &["schema", "doc"], text_outcome(text));
    }
}
