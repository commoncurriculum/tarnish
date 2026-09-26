//! What the real prosemirror-model does with inputs its own tests don't give it, recorded by
//! `harness/record-model.mjs`: tarnish must do the same, from JSON and from Erlang's external
//! term format.

use tarnish::json::{self, Value};
use tarnish::{Fragment, Node, Result, Schema, api, etf};

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
        let bytes = etf::write(json);
        let document = etf::Document::new(&bytes).expect("a term");
        expect(
            case,
            Node::from_json(schema, document.root()).map(|node| node.to_json()),
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
