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

/// `content` inside `depth` blockquotes, as a document's JSON.
fn quoted(depth: usize, content: Value) -> Value {
    let mut node = content;
    for _ in 0..depth {
        node = json!([{ "type": "blockquote", "content": node }]);
    }
    json!({ "type": "doc", "content": node })
}

fn text(text: &str) -> Value {
    json!({ "type": "text", "text": text })
}

fn paragraph(content: Value) -> Value {
    json!({ "type": "paragraph", "content": content })
}

/// Each kind of step, deep in a document: a step writes the node it changes with its list of
/// children, and each ancestor the same way, which gives the document ProseMirror's step gives,
/// read back the same from its chunks. Two steps in one call change the second time the nodes
/// the first wrote, in place.
#[test]
fn each_kind_of_step_deep_in_a_document_gives_the_document_it_should() {
    let fixtures = read("transform");
    let schema = &schemas(&fixtures)[0];
    let depth = 60;
    let heading = |level: u32| json!({ "type": "heading", "attrs": { "level": level }, "content": [text("ab")] });
    let start = quoted(
        depth,
        json!([
            heading(2),
            paragraph(json!([text("abcdef")])),
            paragraph(json!([text("ghij")]))
        ]),
    );
    let doc = Node::from_json(schema, &start).expect("a document");
    let at = |offset: usize| depth + offset;
    let insert = |pos: usize, inserted: &str| json!({ "stepType": "replace", "from": pos, "to": pos, "slice": { "content": [text(inserted)] } });
    let strong = |op: &str| json!({ "stepType": op, "from": at(6), "to": at(8), "mark": { "type": "strong" } });
    let marked = json!([text("a"), { "type": "text", "marks": [{ "type": "strong" }], "text": "bc" }, text("def")]);
    let cases = [
        (
            json!([insert(at(7), "x")]),
            json!([
                heading(2),
                paragraph(json!([text("abxcdef")])),
                paragraph(json!([text("ghij")]))
            ]),
        ),
        (
            json!([insert(at(7), "x"), insert(at(8), "y")]),
            json!([
                heading(2),
                paragraph(json!([text("abxycdef")])),
                paragraph(json!([text("ghij")]))
            ]),
        ),
        (
            json!([strong("addMark")]),
            json!([
                heading(2),
                paragraph(marked.clone()),
                paragraph(json!([text("ghij")]))
            ]),
        ),
        (
            json!([strong("addMark"), strong("removeMark")]),
            json!([
                heading(2),
                paragraph(json!([text("abcdef")])),
                paragraph(json!([text("ghij")]))
            ]),
        ),
        (
            json!([{ "stepType": "attr", "pos": at(0), "attr": "level", "value": 3 }]),
            json!([
                heading(3),
                paragraph(json!([text("abcdef")])),
                paragraph(json!([text("ghij")]))
            ]),
        ),
        (
            json!([{ "stepType": "replace", "from": at(9), "to": at(15) }]),
            json!([heading(2), paragraph(json!([text("abcdij")]))]),
        ),
        (
            json!([{ "stepType": "replaceAround", "from": at(4), "to": at(12), "gapFrom": at(4), "gapTo": at(12), "insert": 1, "slice": { "content": [{ "type": "blockquote" }] }, "structure": true }]),
            json!([heading(2), { "type": "blockquote", "content": [paragraph(json!([text("abcdef")]))] }, paragraph(json!([text("ghij")]))]),
        ),
        (
            json!([{ "stepType": "replace", "from": at(8), "to": at(8), "slice": { "content": [{ "type": "paragraph" }, { "type": "paragraph" }], "openStart": 1, "openEnd": 1 } }]),
            json!([
                heading(2),
                paragraph(json!([text("abc")])),
                paragraph(json!([text("def")])),
                paragraph(json!([text("ghij")]))
            ]),
        ),
    ];
    let kept = vec![doc.chunk().clone()];
    for (steps, content) in cases {
        let changed = api::apply_steps(&doc, &steps).expect("the steps");
        let expected = quoted(depth, content);
        assert_same(&expected, &changed.to_json());
        changed.check().expect("a valid document");
        assert_same(&expected, &reloaded(schema, &changed, &kept).to_json());
        let mut one_at_a_time = doc.clone();
        for step in steps.as_array().expect("steps") {
            one_at_a_time = api::apply_steps(&one_at_a_time, &json!([step])).expect("a step");
        }
        assert!(one_at_a_time == changed, "{}", steps);
    }
    assert_same(&start, &doc.to_json());
}

/// A replace that changes nothing gives the document itself: an empty slice put into an empty
/// paragraph, deep or not, which ProseMirror's `replace` gives back as it was.
#[test]
fn a_replace_that_changes_nothing_gives_the_document_itself() {
    let fixtures = read("transform");
    let schema = &schemas(&fixtures)[0];
    for depth in [0, 1, 30] {
        let start = quoted(
            depth,
            json!([paragraph(json!([text("a")])), { "type": "paragraph" }]),
        );
        let doc = Node::from_json(schema, &start).expect("a document");
        let empty = depth + 4;
        let slice = tarnish::Slice::empty();
        let same = doc.replace(empty, empty, &slice).expect("a replace");
        assert!(same.ptr_eq(&doc), "at depth {depth}");
        let same = doc
            .clone()
            .into_replaced(empty, empty, &slice)
            .expect("a replace");
        assert!(same.ptr_eq(&doc), "at depth {depth}");
        // A slice that ends inside the empty paragraph holds it as it is.
        let sliced = doc.slice(empty - 2, empty, false).expect("a slice");
        let both = json!([{ "type": "paragraph" }, { "type": "paragraph" }]);
        assert_same(&both, &sliced.content().to_json());
        let paragraph = doc.node_at(empty - 1).expect("a position").expect("a node");
        let last = sliced.content().last_child().expect("a child");
        assert!(last.ptr_eq(&paragraph), "at depth {depth}");
    }
}

/// Steps applied in one call to a document only the call holds change its list of children in
/// place, up to a number of chunks, then copy it: past that number, the document is the one
/// the steps give applied one call at a time.
#[test]
fn many_steps_in_one_call_give_the_document_one_call_a_step_gives() {
    let fixtures = read("transform");
    let schema = &schemas(&fixtures)[0];
    let count = 40;
    for depth in [0, 1, 3] {
        let paragraphs = |typed: &str| {
            Value::Array(
                (0..count)
                    .map(|_| paragraph(json!([text(&format!("a{typed}bc"))])))
                    .collect(),
            )
        };
        let doc = Node::from_json(schema, &quoted(depth, paragraphs(""))).expect("a document");
        // The last paragraph first, so that the positions before it stay as they are.
        let steps: Vec<Value> = (0..count)
            .rev()
            .map(|index| {
                let pos = depth + 5 * index + 2;
                json!({ "stepType": "replace", "from": pos, "to": pos, "slice": { "content": [text("x")] } })
            })
            .collect();
        let changed = api::apply_steps(&doc, &Value::Array(steps.clone())).expect("the steps");
        assert_same(&quoted(depth, paragraphs("x")), &changed.to_json());
        changed.check().expect("a valid document");
        let mut one_at_a_time = doc.clone();
        for step in &steps {
            one_at_a_time = api::apply_steps(&one_at_a_time, &json!([step])).expect("a step");
        }
        assert!(one_at_a_time == changed);
        assert_same(&quoted(depth, paragraphs("")), &doc.to_json());
        let kept = vec![doc.chunk().clone()];
        assert_same(
            &changed.to_json(),
            &reloaded(schema, &changed, &kept).to_json(),
        );
    }
}

/// A document read from JSON, which owns its chunk, has its list of children changed in place
/// only when nothing else holds that chunk: not while a clone of the document does, nor a
/// child the replace made, whose chunk imports it.
#[test]
fn a_chunk_read_from_json_changes_in_place_only_when_nothing_else_holds_it() {
    let fixtures = read("transform");
    let schema = &schemas(&fixtures)[0];
    let start = json!({ "type": "doc", "content": [paragraph(json!([text("abc")])), paragraph(json!([text("def")]))] });
    let insert =
        tarnish::Slice::from_json(schema, &json!({ "content": [text("x")] })).expect("a slice");
    let doc = Node::from_json(schema, &start).expect("a document");
    let bytes = doc.chunk().bytes().to_vec();
    // A clone holds the chunk.
    let held = doc.clone();
    let changed = doc.into_replaced(2, 2, &insert).expect("a replace");
    assert_eq!(held.chunk().bytes(), bytes.as_slice());
    assert_same(&start, &held.to_json());
    let typed = json!({ "type": "doc", "content": [paragraph(json!([text("axbc")])), paragraph(json!([text("def")]))] });
    assert_same(&typed, &changed.to_json());
    // The typed paragraph refers to the text it kept, so its chunk imports the read one.
    let changed = held.into_replaced(2, 2, &insert).expect("a replace");
    assert_same(&typed, &changed.to_json());
    let mut imported = vec![changed.chunk().clone()];
    let mut found = false;
    while let Some(chunk) = imported.pop() {
        found |= chunk.bytes() == bytes.as_slice();
        imported.extend(chunk.imports().iter().cloned());
    }
    assert!(found, "the read chunk among the imports");
    // A paragraph's content replaced whole by the slice's shares nothing with the read chunk,
    // which nothing else holds, so its list changes in place.
    let doc = Node::from_json(schema, &start).expect("a document");
    let changed = doc.into_replaced(1, 4, &insert).expect("a replace");
    let replaced = json!({ "type": "doc", "content": [paragraph(json!([text("x")])), paragraph(json!([text("def")]))] });
    assert_same(&replaced, &changed.to_json());
    assert_eq!(
        changed.chunk().imports().len(),
        1,
        "the read chunk, importing the paragraph's"
    );
    changed.check().expect("a valid document");
    assert_same(&replaced, &reloaded(schema, &changed, &[]).to_json());
}
