//! What the real ProseMirror packages do with inputs their own tests don't give them, recorded
//! by `harness/record-model.mjs`: tarnish must do the same.

use tarnish::json::{self, Value, json};
use tarnish::transform::{Step, Transform};
use tarnish::{Fragment, Node, Schema, api};
use tarnish_fixtures::{assert_same, expect, mapped, outcome, read, records, schemas};

fn fixtures() -> (Vec<Schema>, Value) {
    let fixtures = read("model");
    (schemas(&fixtures), fixtures)
}

fn schema<'a>(schemas: &'a [Schema], case: &Value) -> &'a Schema {
    &schemas[case["schema"].as_u64().expect("a schema's index") as usize]
}

/// A record's JSON, as its `text` where it has one: numbers spelled as JSON.stringify wouldn't.
fn given(case: &Value) -> Value {
    match case.get("text") {
        Some(text) => json::from_str(text.as_str().expect("JSON text")).expect("JSON"),
        None => case["json"].clone(),
    }
}

#[test]
fn nodes_from_json_are_prosemirrors() {
    let (schemas, fixtures) = fixtures();
    for case in records(&fixtures, "fromJSON") {
        let node = Node::from_json(schema(&schemas, case), &given(case));
        let node = node.map(|node| json!({ "result": node.to_json() }));
        expect(case, &["schema", "json", "text"], outcome(node));
    }
}

/// The same records read from their text, as `Node.fromJSON(schema, JSON.parse(text))`.
#[test]
fn nodes_from_json_text_are_prosemirrors() {
    let (schemas, fixtures) = fixtures();
    for case in records(&fixtures, "fromJSON") {
        let text = match case.get("text") {
            Some(text) => text.as_str().expect("JSON text").to_string(),
            None => json::stringify(&case["json"]),
        };
        let node = Node::from_json_text(schema(&schemas, case), &text);
        let node = node.map(|node| json!({ "result": node.to_json() }));
        expect(case, &["schema", "json", "text"], outcome(node));
    }
}

#[test]
fn filled_nodes_are_prosemirrors() {
    let (schemas, fixtures) = fixtures();
    for case in records(&fixtures, "createAndFill") {
        let filled =
            schema(&schemas, case)
                .top_node_type()
                .create_and_fill(None, Fragment::empty(), &[]);
        let filled = filled.map(|node| node.map_or(Value::Null, |node| node.to_json()));
        let filled = filled.map(|filled| json!({ "result": filled }));
        expect(case, &["schema"], outcome(filled));
    }
}

#[test]
fn schemas_and_their_errors_are_prosemirrors() {
    let (_, fixtures) = fixtures();
    for case in records(&fixtures, "newSchema") {
        let schema = api::schema(&case["spec"]).map(|_| json!({ "result": null }));
        expect(case, &["spec"], outcome(schema));
    }
}

#[test]
fn checks_are_prosemirrors() {
    let (schemas, fixtures) = fixtures();
    for case in records(&fixtures, "check") {
        let doc = Node::from_json(schema(&schemas, case), &case["json"]);
        let checked = doc
            .and_then(|doc| doc.check())
            .map(|()| json!({ "result": null }));
        expect(case, &["schema", "json"], outcome(checked));
    }
}

/// Steps that split surrogate pairs leave ProseMirror's lone surrogates, which JSON, holding none,
/// has as U+FFFD, each in its place; steps that fail, fail with ProseMirror's errors.
#[test]
fn steps_give_prosemirrors_text_and_errors() {
    let (schemas, fixtures) = fixtures();
    for case in records(&fixtures, "transforms") {
        let schema = schema(&schemas, case);
        let doc = Node::from_json(schema, &case["start"]).expect("a document");
        let inputs = ["schema", "start", "steps", "wellFormed"];
        let changed = match api::apply_steps(&doc, &case["steps"]) {
            Ok(changed) => changed,
            Err(error) => {
                expect(case, &inputs, outcome::<Value>(Err(error)));
                continue;
            }
        };
        let written = json!({
            "result": changed.to_json_string(),
            "inverted": api::invert_steps(&doc, &case["steps"]).expect("their inverse"),
            "mapped": mapped(schema, &case["steps"], doc.content().size()),
        });
        expect(case, &inputs, written);
        let text = json::from_str(case["result"].as_str().expect("JSON text")).expect("JSON");
        let read = Node::from_json(schema, &text).expect("a document");
        assert_same(case.get("wellFormed").unwrap_or(&text), &read.to_json());

        // The steps themselves, which keep their lone surrogates, undo the change exactly.
        let mut tr = Transform::new(doc.clone());
        for step in case["steps"].as_array().expect("steps") {
            tr.step(Step::from_json(schema, step).expect("a step"))
                .expect("the step");
        }
        let mut undo = Transform::new(tr.doc().clone());
        for (step, before) in tr.steps().iter().zip(tr.docs()).rev() {
            undo.step(step.invert(before).expect("an inverse"))
                .expect("the inverse");
        }
        assert!(*undo.doc() == doc, "{}", undo.doc().to_json_string());
    }
}
