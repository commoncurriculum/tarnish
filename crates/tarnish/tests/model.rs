//! What the real ProseMirror packages do with inputs their own tests don't give them, recorded
//! by `harness/record-model.mjs`: tarnish must do the same.

use tarnish::json::{self, Value};
use tarnish::{Fragment, Node, Result, Schema, api};

fn fixtures() -> (Vec<Schema>, Value) {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/model.json");
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

fn schema<'a>(schemas: &'a [Schema], case: &Value) -> &'a Schema {
    &schemas[case["schema"].as_u64().expect("a schema's index") as usize]
}

fn expect(case: &Value, outcome: Result<Value>) {
    match outcome {
        Ok(result) => assert_eq!(Some(&result), case.get("result"), "{case}"),
        Err(error) => {
            let recorded = &case["error"];
            assert_eq!(error.class(), recorded["class"], "{case}");
            assert_eq!(error.message(), recorded["message"], "{case}");
        }
    }
}

#[test]
fn nodes_from_json_are_prosemirrors() {
    let (schemas, fixtures) = fixtures();
    for case in fixtures["fromJSON"].as_array().expect("cases") {
        let schema = schema(&schemas, case);
        let json = &case["json"];
        expect(
            case,
            Node::from_json(schema, json).map(|node| node.to_json()),
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
        expect(
            case,
            filled.map(|node| node.map_or(Value::Null, |node| node.to_json())),
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
    }
}
