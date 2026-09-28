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

/// `node` flattened into a chunk that refers into `kept`, read back from its bytes, as a
/// binding that keeps chunks as bytes reads it.
fn reloaded(
    schema: &Schema,
    node: &Node<'static>,
    kept: &[std::sync::Arc<tarnish::chunk::Chunk<'static>>],
) -> Node<'static> {
    let flat = node.flatten(kept);
    let bytes = flat.chunk().bytes().to_vec();
    let imports = tarnish::chunk::Header::read(&bytes)
        .expect("a header")
        .imports
        .iter()
        .map(|id| {
            let import = kept.iter().find(|chunk| chunk.id() == *id);
            import.expect("an import that is kept").clone()
        })
        .collect();
    let chunk = tarnish::chunk::Chunk::load(std::borrow::Cow::Owned(bytes), schema, imports)
        .expect("a chunk");
    Node::root(std::sync::Arc::new(chunk)).expect("a node")
}

/// A change written into one chunk more, which refers into the chunks before it, is the same
/// document read back from the chunks' bytes, and so is the change undoing it, on three.
#[test]
fn changes_read_back_from_their_chunks() {
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
        let read = Node::from_json(schema, &case["start"]).expect("a document");
        let source = reloaded(schema, &read, &[]);
        let mut kept = vec![source.chunk().clone()];
        let changed = api::apply_steps(&source, &case["steps"]).expect("the steps");
        let changed = reloaded(schema, &changed, &kept);
        assert!(changed.to_json() == case["result"], "{case}");
        assert!(changed.compact().to_json() == case["result"], "{case}");
        kept.push(changed.chunk().clone());
        let inverted = api::invert_steps(&source, &case["steps"]).expect("the inverted steps");
        let undone = api::apply_steps(&changed, &inverted).expect("the steps undone");
        let undone = reloaded(schema, &undone, &kept);
        assert!(undone.to_json() == case["start"], "{case}");
        assert!(undone.check().is_ok(), "{case}");
    }
}
