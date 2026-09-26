//! A document nested far deeper than a small stack could recurse through, and every operation
//! on it, on a thread with such a stack: each recursion over nesting has to grow the stack.

use tarnish::json::{self, Value};
use tarnish::transform::{BlockAttrs, Step, Transform, Wrapper};
use tarnish::{Node, Schema, Slice, api};

const DEPTH: usize = 20_000;

fn on_small_stack(test: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(256 << 10)
        .spawn(test)
        .expect("a thread")
        .join()
        .expect("the test");
}

fn schema() -> Schema {
    api::schema(&json::from_str(SCHEMA).expect("the spec")).expect("the schema")
}

const SCHEMA: &str = r#"{
    "nodes": [
        ["doc", {"content": "block+"}],
        ["paragraph", {"content": "inline*", "group": "block"}],
        ["blockquote", {"content": "block+", "group": "block", "attrs": {"data": {"default": null}}}],
        ["text", {"group": "inline"}]
    ],
    "marks": [["em", {}], ["link", {"attrs": {"href": {}}}]]
}"#;

/// `levels` arrays, one inside another.
fn nested_array(levels: usize) -> Value {
    (0..levels).fold(Value::Array(Vec::new()), |inner, _| {
        Value::Array(vec![inner])
    })
}

fn object(entries: Vec<(&str, Value)>) -> Value {
    Value::Object(
        entries
            .into_iter()
            .map(|(key, value)| (key.into(), value))
            .collect(),
    )
}

/// A paragraph inside `DEPTH` blockquotes, the innermost holding a deeply nested attribute.
fn deep_json() -> Value {
    let text = object(vec![("type", "text".into()), ("text", "deep text".into())]);
    let paragraph = object(vec![
        ("type", "paragraph".into()),
        ("content", Value::Array(vec![text])),
    ]);
    let innermost = object(vec![
        ("type", "blockquote".into()),
        ("attrs", object(vec![("data", nested_array(DEPTH))])),
        ("content", Value::Array(vec![paragraph])),
    ]);
    let quotes = (1..DEPTH).fold(innermost, |inner, _| {
        object(vec![
            ("type", "blockquote".into()),
            ("content", Value::Array(vec![inner])),
        ])
    });
    object(vec![
        ("type", "doc".into()),
        ("content", Value::Array(vec![quotes])),
    ])
}

/// Where the deep paragraph's text starts.
const TEXT: usize = DEPTH + 1;

#[test]
fn documents_nest_as_deeply_as_memory_allows() {
    on_small_stack(|| {
        let schema = schema();
        let json = deep_json();
        let doc = Node::from_json(&schema, &json).expect("the document");
        doc.check().expect("a valid document");
        let written = doc.to_json();
        let again = Node::from_json(&schema, &written).expect("the document again");
        assert!(doc == again);
        assert!(again.to_json() == written);
        assert!(
            doc.to_debug_string()
                .expect("its string")
                .contains("deep text")
        );
        assert_eq!(
            doc.text_content().expect("its text").to_string(),
            "deep text"
        );
        let mut visited = 0;
        doc.descendants(&mut |_, _, _, _| {
            visited += 1;
            Ok(true)
        })
        .expect("its descendants");
        assert_eq!(visited, DEPTH + 2);
        assert_eq!(
            doc.resolve(TEXT + 4).expect("a position").depth(),
            DEPTH + 1
        );

        let slice = doc.slice(TEXT + 2, TEXT + 6, false).expect("a slice");
        let slice_json = slice.to_json();
        assert!(
            Slice::from_json(&schema, &slice_json)
                .expect("the slice again")
                .to_json()
                == slice_json
        );
        let replaced = doc
            .replace(TEXT + 1, TEXT + 3, &Slice::empty())
            .expect("a replace");
        assert_eq!(
            doc.content().find_diff_start(replaced.content(), 0),
            Some(TEXT + 1)
        );
        assert!(
            doc.content()
                .find_diff_end(
                    replaced.content(),
                    doc.content().size(),
                    replaced.content().size()
                )
                .is_some()
        );

        let em = schema
            .mark(&schema.mark_type("em").expect("em"), None)
            .expect("em");
        let link = |href| {
            let attrs = object(vec![("href", href)]);
            schema
                .mark(&schema.mark_type("link").expect("link"), attrs.as_object())
                .expect("a link")
        };
        let paragraph = schema.node_type("paragraph").expect("paragraph");
        let blockquote = schema.node_type("blockquote").expect("blockquote");
        let mut tr = Transform::new(doc.clone());
        tr.add_mark(TEXT, TEXT + 4, &em)
            .expect("add em")
            .add_mark(TEXT, TEXT + 4, &link(nested_array(DEPTH)))
            .expect("add a link")
            .add_mark(TEXT + 2, TEXT + 6, &link(nested_array(DEPTH)))
            .expect("add an equal link")
            .remove_mark(TEXT, TEXT + 2, None)
            .expect("remove marks")
            .split(TEXT + 4, DEPTH + 1, &[])
            .expect("split every level");
        let split_at = TEXT + 4 + DEPTH + 1;
        tr.join(split_at, DEPTH + 1).expect("join them again");
        let range = tr
            .doc()
            .resolve(TEXT)
            .expect("a position")
            .block_range(&tr.doc().resolve(TEXT + 4).expect("a position"), None)
            .expect("a range")
            .expect("a block range");
        tr.wrap(
            &range,
            &[Wrapper {
                node_type: blockquote.clone(),
                attrs: None,
            }],
        )
        .expect("wrap");
        let range = tr
            .doc()
            .resolve(TEXT + 1)
            .expect("a position")
            .block_range(&tr.doc().resolve(TEXT + 5).expect("a position"), None)
            .expect("a range")
            .expect("a block range");
        tr.lift(&range, DEPTH).expect("lift");
        tr.set_block_type(
            0,
            tr.doc().content().size(),
            &paragraph,
            BlockAttrs::Fixed(None),
        )
        .expect("set block type")
        .set_node_markup(DEPTH - 1, None, None, None)
        .expect("set node markup")
        .delete_range(TEXT, TEXT + 2)
        .expect("delete range")
        .replace_range(
            TEXT,
            TEXT,
            &doc.slice(TEXT + 2, TEXT + 6, true).expect("a slice"),
        )
        .expect("replace range");
        tr.doc().check().expect("a valid document");

        let steps: Vec<Value> = tr.steps().iter().map(Step::to_json).collect();
        let steps = Value::Array(steps);
        let applied = api::apply_steps(&doc, &steps).expect("the steps");
        assert!(applied == *tr.doc());
        let inverted = api::invert_steps(&doc, &steps).expect("their inverse");
        assert!(api::apply_steps(&applied, &inverted).expect("the inverse") == doc);
        let mut undone = tr.doc().clone();
        for (step, before) in tr.steps().iter().zip(tr.docs()).rev() {
            let inverted = step.invert(before).expect("an inverted step");
            let inverted = Step::from_json(&schema, &inverted.to_json()).expect("the step again");
            undone = inverted
                .apply(&undone)
                .expect("applies")
                .doc()
                .expect("a document")
                .clone();
        }
        assert!(undone == doc);
    });
}

#[test]
fn content_expressions_nest_as_deeply_as_memory_allows() {
    on_small_stack(|| {
        let content = format!("{}block{}+", "(".repeat(DEPTH), ")".repeat(DEPTH));
        let spec = SCHEMA.replace(
            r#""content": "block+""#,
            &format!(r#""content": "{content}""#),
        );
        let schema = api::schema(&json::from_str(&spec).expect("the spec")).expect("the schema");
        let paragraph = object(vec![("type", "paragraph".into())]);
        let doc = object(vec![
            ("type", "doc".into()),
            ("content", Value::Array(vec![paragraph])),
        ]);
        Node::from_json(&schema, &doc)
            .and_then(|doc| doc.check())
            .expect("a valid document");
    });
}
