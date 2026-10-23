//! Transform ops, and `textBetween` and `textContent`, as the real ProseMirror packages ran
//! them, recorded by `harness/record-ops.mjs`: tarnish must give the same documents, steps,
//! mapped positions, text and errors.

use tarnish::json::{Value, json};
use tarnish::transform::Step;
use tarnish::{Node, Result, Schema, Text, api};
use tarnish_fixtures::{expect, mapped, outcome, read, records, schemas};

fn fixtures() -> (Vec<Schema>, Value) {
    let fixtures = read("ops");
    (schemas(&fixtures), fixtures)
}

fn schema<'a>(schemas: &'a [Schema], case: &Value) -> &'a Schema {
    &schemas[case["schema"].as_u64().expect("a schema's index") as usize]
}

fn doc(schemas: &[Schema], case: &Value) -> Node<'static> {
    Node::from_json(schema(schemas, case), &case["doc"]).expect("a document")
}

/// Text with a lone surrogate is recorded as its JSON.
fn written(text: Text) -> Value {
    match text.as_str() {
        Some(text) => text.into(),
        None => json!({ "resultJSON": text.to_json_string() }),
    }
}

fn text_outcome(text: Result<Text>) -> Value {
    outcome(text.map(|text| {
        let written = written(text);
        match written.is_string() {
            true => json!({ "result": written }),
            false => written,
        }
    }))
}

fn separators(case: &Value) -> (Option<&str>, Option<&str>) {
    let given = |key: &str| case.get(key).and_then(Value::as_str);
    (given("blockSeparator"), given("leafText"))
}

#[test]
fn ops_give_prosemirrors_document_steps_mapping_and_errors() {
    let (schemas, fixtures) = fixtures();
    for case in records(&fixtures, "transforms") {
        let doc = doc(&schemas, case);
        let before = doc.to_json().to_value();
        let schema = schema(&schemas, case);
        let transformed = api::transform(&doc, &case["ops"]).map(|(changed, steps)| {
            // `==` is `node.eq`, which ProseMirror's own suites hold the port's documents to.
            let eq = changed == doc;
            let mut given =
                json!({ "result": changed.to_json(), "steps": steps.clone(), "eq": eq });
            // Mapped where the steps read back from their JSON, as `map_position` reads them: a
            // step may hold a node whose attributes `Node.fromJSON` refuses.
            let read = steps.as_array().expect("steps").iter();
            if read
                .map(|step| Step::from_json(schema, step))
                .all(|step| step.is_ok())
            {
                given["mapped"] = mapped(schema, &steps, doc.content().size());
            }
            given
        });
        expect(case, &["schema", "doc", "ops"], outcome(transformed));
        assert!(doc.to_json() == before, "{case}");
    }
}

/// Past the end of the document, and with an end before the start.
#[test]
fn text_between_is_prosemirrors() {
    let (schemas, fixtures) = fixtures();
    for case in records(&fixtures, "textBetween") {
        let position = |key: &str| case[key].as_u64().expect("a position") as usize;
        let (separator, leaf) = separators(case);
        let doc = doc(&schemas, case);
        let text = api::text_between(&doc, position("from"), position("to"), separator, leaf);
        let inputs = ["schema", "doc", "from", "to", "blockSeparator", "leafText"];
        expect(case, &inputs, text_outcome(text));
    }
}

#[test]
fn text_between_every_two_positions_is_prosemirrors() {
    let (schemas, fixtures) = fixtures();
    for case in records(&fixtures, "textsBetween") {
        let (separator, leaf) = separators(case);
        let doc = doc(&schemas, case);
        let size = doc.text().map_or(doc.content().size(), |_| doc.node_size());
        let texts: Vec<Value> = (0..=size)
            .map(|from| {
                let between = (from..=size).map(|to| {
                    let text = api::text_between(&doc, from, to, separator, leaf);
                    written(text.expect("the text"))
                });
                Value::Array(between.collect())
            })
            .collect();
        expect(
            case,
            &["schema", "doc", "blockSeparator", "leafText"],
            json!({ "texts": texts }),
        );
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
