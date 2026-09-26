//! Reading nodes from Erlang's external term format in place gives the nodes reading their JSON
//! gives, for every document the recorded transforms start from or end with.

use tarnish::json::{self, Value};
use tarnish::{Node, api, etf};

#[test]
fn nodes_read_in_place_are_the_nodes_read_from_json() {
    let fixtures: Value = json::from_str(
        &std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/transform.json"
        ))
        .expect("the fixtures"),
    )
    .expect("JSON");
    let schemas: Vec<_> = fixtures["schemas"]
        .as_array()
        .expect("schemas")
        .iter()
        .map(|spec| api::schema(spec).expect("a schema"))
        .collect();
    for test in fixtures["tests"].as_array().expect("tests") {
        let schema = &schemas[test["schema"].as_u64().expect("a schema's index") as usize];
        for json in [&test["start"], &test["result"]] {
            let bytes = etf::write(json);
            let document = etf::Document::new(&bytes).expect("a term");
            let read = Node::from_json(schema, document.root()).expect("a node");
            assert!(read == Node::from_json(schema, json).expect("a node"));
            assert!(etf::read(&etf::write_node(&read)).expect("a term") == read.to_json());
        }
    }
}
