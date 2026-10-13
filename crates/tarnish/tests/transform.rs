//! The transforms `prosemirror-transform`'s tests make, recorded from the real package by
//! `npm run test:js`, with what `harness/record-model.mjs` records of each: the steps undoing it,
//! and where every position maps through it.

use tarnish::json::{Value, json};
use tarnish::{Node, Schema, api};
use tarnish_fixtures::{assert_same, expect, mapped, read, records, schemas};

fn schema<'a>(schemas: &'a [Schema], case: &Value) -> &'a Schema {
    &schemas[case["schema"].as_u64().expect("a schema's index") as usize]
}

/// Steps after the first are applied to a document only `apply_steps` holds, which a replace
/// changes in place: the document it was given must stay as it was.
#[test]
fn steps_give_prosemirrors_document_inverse_and_mapping_and_leave_the_one_given() {
    let fixtures = read("transform");
    let model = read("model");
    let schemas = schemas(&fixtures);
    let tests = records(&fixtures, "tests");
    let recorded = records(&model, "transformTests");
    assert!(tests.len() == recorded.len(), "a record for each test");
    for (index, (case, recorded)) in tests.iter().zip(recorded).enumerate() {
        assert_same(&json!(index), &recorded["test"]);
        let schema = schema(&schemas, case);
        let doc = Node::from_json(schema, &case["start"]).expect("a document");
        let changed = api::apply_steps(&doc, &case["steps"]).expect("the steps");
        let tagged: Vec<Value> = case["mapping"]
            .as_array()
            .expect("positions")
            .iter()
            .map(|pair| {
                let from = pair[0].as_u64().expect("a position") as usize;
                let to = api::map_position(schema, &case["steps"], from, 1).expect("a position");
                json!([from, to])
            })
            .collect();
        let given = json!({ "result": changed.to_json(), "mapping": tagged });
        expect(case, &["schema", "start", "steps"], given);
        // `==` is `node.eq`, which ProseMirror's own suites hold the port's documents to.
        let given = json!({
            "inverted": api::invert_steps(&doc, &case["steps"]).expect("the inverted steps"),
            "eq": changed == doc,
            "mapped": mapped(schema, &case["steps"], doc.content().size()),
        });
        expect(recorded, &["test"], given);
        let read = Node::from_json(schema, &changed.to_json()).expect("a document");
        assert!(read == changed, "{}", changed.to_json());
        assert_same(&case["start"], &doc.to_json());
        let again = api::apply_steps(&doc, &case["steps"]).expect("the steps again");
        assert_same(&case["result"], &again.to_json());
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
    let fixtures = read("transform");
    let schemas = schemas(&fixtures);
    for case in records(&fixtures, "tests") {
        let schema = schema(&schemas, case);
        let read = Node::from_json(schema, &case["start"]).expect("a document");
        let source = reloaded(schema, &read, &[]);
        let mut kept = vec![source.chunk().clone()];
        let changed = api::apply_steps(&source, &case["steps"]).expect("the steps");
        let changed = reloaded(schema, &changed, &kept);
        assert_same(&case["result"], &changed.to_json());
        assert_same(&case["result"], &changed.compact().to_json());
        kept.push(changed.chunk().clone());
        let inverted = api::invert_steps(&source, &case["steps"]).expect("the inverted steps");
        let undone = api::apply_steps(&changed, &inverted).expect("the steps undone");
        let undone = reloaded(schema, &undone, &kept);
        assert_same(&case["start"], &undone.to_json());
        undone.check().expect("a valid document");
    }
}
