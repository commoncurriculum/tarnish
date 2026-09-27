//! The transforms `prosemirror-transform`'s tests make, recorded from the real package by
//! `npm run test:js`.

use tarnish::json::{self, Value};
use tarnish::{Node, Schema, api};

/// Steps after the first are applied to a document only `apply_steps` holds, which a replace
/// changes in place: the document it was given must stay as it was.
#[test]
fn steps_give_prosemirrors_document_and_leave_the_one_given() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/transform.json");
    let fixtures =
        json::from_str(&std::fs::read_to_string(path).expect("the fixtures")).expect("JSON");
    let schemas: Vec<Schema> = fixtures["schemas"]
        .as_array()
        .expect("schemas")
        .iter()
        .map(|spec| api::schema(spec).expect("a schema"))
        .collect();
    for case in fixtures["tests"].as_array().expect("tests") {
        let schema = &schemas[case["schema"].as_u64().expect("a schema's index") as usize];
        let doc = Node::from_json(schema, &case["start"]).expect("a document");
        let changed = api::apply_steps(&doc, &case["steps"]).expect("the steps");
        assert!(changed.to_json() == case["result"], "{case}");
        assert!(doc.to_json() == case["start"], "{case}");
        let again: Value = api::apply_steps(&doc, &case["steps"])
            .expect("the steps again")
            .to_json();
        assert!(again == case["result"], "{case}");
    }
}
