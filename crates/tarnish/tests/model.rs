//! What the real ProseMirror packages do with inputs their own tests don't give them, recorded
//! by `harness/record-model.mjs`: tarnish must do the same.

use tarnish::json::{self, Value, json};
use tarnish::{Fragment, Node, Schema, api};
use tarnish_fixtures::{expect, outcome, read, schemas};

fn fixtures() -> (Vec<Schema>, Value) {
    let fixtures = read("model");
    (schemas(&fixtures), fixtures)
}

fn schema<'a>(schemas: &'a [Schema], case: &Value) -> &'a Schema {
    &schemas[case["schema"].as_u64().expect("a schema's index") as usize]
}

#[test]
fn nodes_from_json_are_prosemirrors() {
    let (schemas, fixtures) = fixtures();
    for case in fixtures["fromJSON"].as_array().expect("cases") {
        let node = Node::from_json(schema(&schemas, case), &case["json"]);
        expect(
            case,
            outcome(node.map(|node| json!({ "result": node.to_json() }))),
        );
    }
}

#[test]
fn filled_nodes_are_prosemirrors() {
    let (schemas, fixtures) = fixtures();
    for case in fixtures["createAndFill"].as_array().expect("cases") {
        let filled =
            schema(&schemas, case)
                .top_node_type()
                .create_and_fill(None, Fragment::empty(), &[]);
        let filled = filled.map(|node| node.map_or(Value::Null, |node| node.to_json()));
        expect(
            case,
            outcome(filled.map(|filled| json!({ "result": filled }))),
        );
    }
}

#[test]
fn steps_that_split_a_surrogate_pair_leave_prosemirrors_text() {
    let (schemas, fixtures) = fixtures();
    for case in fixtures["transforms"].as_array().expect("cases") {
        let doc = Node::from_json(schema(&schemas, case), &case["start"]).expect("a document");
        let changed = api::apply_steps(&doc, &case["steps"]).expect("the steps");
        assert_eq!(
            Some(changed.to_json_string().as_str()),
            case["result"].as_str()
        );
        let inverted = api::invert_steps(&doc, &case["steps"]).expect("their inverse");
        let undone = api::apply_steps(&changed, &inverted).expect("the inverse");
        assert!(undone == doc);
        // The text, lone surrogates and all, reads back, each one as U+FFFD in its place.
        let written = json::from_str(case["result"].as_str().expect("JSON text")).expect("JSON");
        let read = Node::from_json(schema(&schemas, case), &written).expect("a document");
        assert_eq!(read.content().size(), changed.content().size());
    }
}
