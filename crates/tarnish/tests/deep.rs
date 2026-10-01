//! A document nested far deeper than a small stack could recurse through, and every operation
//! on it, on a thread with such a stack: each recursion over nesting has to grow the stack.

use tarnish::js::stack::on_dirty_scheduler_stack;
use tarnish::json::{self, Value};
use tarnish::transform::{BlockAttrs, Step, Transform, Wrapper};
use tarnish::{Fragment, Node, Schema, Slice, api};

const DEPTH: usize = 20_000;

/// How many node types a schema chains, each holding the next, whose names it looks up and
/// whose generation it checks against those it's in, in time that grows with the square of
/// their number.
const TYPES: usize = 2_000;

/// How deep a slice is open where a transform fits it, closing each level in time that grows
/// with the depth.
const FITTED: usize = 3_000;

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

/// `levels` objects, one inside another.
fn nested_object(levels: usize) -> Value {
    (0..levels).fold(Value::Null, |inner, _| object(vec![("a", inner)]))
}

/// `levels` objects and arrays, each inside the other.
fn alternating(levels: usize) -> Value {
    (0..levels).fold(Value::Null, |inner, level| match level % 2 {
        0 => object(vec![("a", inner)]),
        _ => Value::Array(vec![inner]),
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
    on_dirty_scheduler_stack(|| {
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
                .apply(undone)
                .expect("applies")
                .doc()
                .expect("a document")
                .clone();
        }
        assert!(undone == doc);
    });
}

/// An attribute's value, as JavaScript turns it into a string or a number, which joins each
/// array inside it, and as `compareDeep` compares it, arrays and objects alike.
#[test]
fn attribute_values_nest_as_deeply_as_memory_allows() {
    on_dirty_scheduler_stack(|| {
        let schema = schema();
        let quote = |data: Value| {
            let paragraph = object(vec![("type", "paragraph".into())]);
            let quote = object(vec![
                ("type", "blockquote".into()),
                ("attrs", object(vec![("data", data)])),
                ("content", Value::Array(vec![paragraph])),
            ]);
            Node::from_json(&schema, &quote).expect("a quote")
        };
        let arrays = quote(nested_array(DEPTH));
        let data = arrays.attrs_view().get("data").expect("its data");
        assert_eq!(data.to_js_string().expect("a string"), "");
        assert_eq!(data.to_number().expect("a number"), 0.0);

        assert!(quote(alternating(DEPTH)) == quote(alternating(DEPTH)));
        assert!(quote(alternating(DEPTH)) != arrays);

        // `JSON.stringify` of the node, and `{:?}` of its attributes and of a mark's.
        for data in [
            nested_array(DEPTH),
            nested_object(DEPTH),
            alternating(DEPTH),
        ] {
            let node = quote(data.clone());
            let written = json::from_str(&node.to_json_string()).expect("its JSON");
            assert!(written == node.to_json());
            let attrs = object(vec![("data", data)]);
            let formatted = format!("{:?}", node.attrs());
            assert!(json::from_str(&formatted).expect("JSON") == attrs);

            let link_type = schema.mark_type("link").expect("link");
            let attrs = object(vec![("href", attrs)]);
            let link = schema.mark(&link_type, attrs.as_object()).expect("a link");
            let formatted = format!("{link:?}");
            let formatted = formatted.strip_prefix("link").expect("the mark's name");
            assert!(json::from_str(formatted).expect("JSON") == attrs);
        }
    });
}

#[test]
fn content_expressions_nest_as_deeply_as_memory_allows() {
    on_dirty_scheduler_stack(|| {
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

#[test]
fn repetitions_nest_as_deeply_as_memory_allows() {
    on_dirty_scheduler_stack(|| {
        let content = format!("{}block{}", "(".repeat(DEPTH), ")*".repeat(DEPTH));
        let spec = SCHEMA.replace(
            r#""content": "block+""#,
            &format!(r#""content": "{content}""#),
        );
        let schema = api::schema(&json::from_str(&spec).expect("the spec")).expect("the schema");
        let doc = schema
            .top_node_type()
            .create_and_fill(None, Fragment::empty(), &[])
            .expect("no error")
            .expect("a document");
        assert_eq!(doc.child_count(), 0);
    });
}

#[test]
fn content_may_hold_as_many_nodes_as_memory_allows() {
    on_dirty_scheduler_stack(|| {
        let spec = SCHEMA.replace(
            r#""content": "block+""#,
            &format!(r#""content": "paragraph{{{DEPTH}}}""#),
        );
        let schema = api::schema(&json::from_str(&spec).expect("the spec")).expect("the schema");
        let doc = schema
            .top_node_type()
            .create_and_fill(None, Fragment::empty(), &[])
            .expect("no error")
            .expect("a document");
        assert_eq!(doc.child_count(), DEPTH);
        doc.check().expect("a valid document");
        let states = schema.top_node_type().content_match().to_string();
        assert_eq!(states.lines().count(), DEPTH + 1);
    });
}

/// What becomes of a document a transform changed: it is written again into a chunk of its own,
/// or into one that refers to the chunks it was read into, as the NIF hands it back, and written
/// as JSON text.
#[test]
fn changed_documents_are_written_as_deeply_as_memory_allows() {
    on_dirty_scheduler_stack(|| {
        let schema = schema();
        let doc = Node::from_json(&schema, &deep_json()).expect("the document");
        let em = schema
            .mark(&schema.mark_type("em").expect("em"), None)
            .expect("em");
        let mut tr = Transform::new(doc.clone());
        tr.add_mark(TEXT, TEXT + 4, &em).expect("add em");
        let changed = tr.doc().clone();

        let compact = changed.compact();
        assert!(compact == changed);
        assert!(compact.to_json() == changed.to_json());
        let flat = changed.flatten(std::slice::from_ref(doc.chunk()));
        assert!(flat == changed);

        let written = changed.to_json_string();
        assert!(json::from_str(&written).expect("its JSON") == changed.to_json());
    });
}

/// A schema whose types each hold the next, as many as memory allows: filling the first
/// generates each of the others inside it.
#[test]
fn content_fills_through_as_many_node_types_as_memory_allows() {
    on_dirty_scheduler_stack(|| {
        let mut nodes: Vec<Value> = (0..TYPES)
            .map(|level| {
                let content = format!("t{}", level + 1);
                json::json!([format!("t{level}"), {"content": content}])
            })
            .collect();
        nodes.push(json::json!([format!("t{TYPES}"), {"content": "text*"}]));
        nodes.push(json::json!(["text", {}]));
        let spec = json::json!({"nodes": nodes, "topNode": "t0"});
        let schema = api::schema(&spec).expect("the schema");
        let filled = schema
            .top_node_type()
            .create_and_fill(None, Fragment::empty(), &[])
            .expect("no error")
            .expect("a node");
        assert_eq!(first_depth(&filled), TYPES);
    });
}

/// How many first children deep the node goes, walked without recursing.
fn first_depth(node: &Node<'static>) -> usize {
    let mut depth = 0;
    let mut node = node.clone();
    while let Some(child) = node.first_child() {
        depth += 1;
        node = child;
    }
    depth
}

/// A document holding one paragraph, `x`.
fn shallow(schema: &Schema) -> Node<'static> {
    let paragraph = json::json!({"type": "paragraph", "content": [{"type": "text", "text": "x"}]});
    let doc = json::json!({"type": "doc", "content": [paragraph]});
    Node::from_json(schema, &doc).expect("the document")
}

/// A slice of `deep text` from inside `levels` nested `inner` nodes, open at every level, put at
/// the top of a shallow document, which takes only its outermost node, whose start each level
/// down is closed.
fn fit_at_the_top(levels: usize) -> Node<'static> {
    let spec = json::json!({"nodes": [
        ["doc", {"content": "outer+"}],
        ["outer", {"content": "inner"}],
        ["inner", {"content": "inner | paragraph"}],
        ["paragraph", {"content": "text*"}],
        ["text", {}],
    ]});
    let schema = api::schema(&spec).expect("the schema");
    let paragraph = |text: &str| json::json!({"type": "paragraph", "content": [{"type": "text", "text": text}]});
    let inner = |content: Value| json::json!({"type": "inner", "content": [content]});
    let outer = |content: Value| json::json!({"type": "doc", "content": [{"type": "outer", "content": [content]}]});
    let nested = (0..levels).fold(paragraph("deep text"), |content, _| inner(content));
    let deep = Node::from_json(&schema, &outer(nested)).expect("the deep document");
    let text = levels + 2;
    let slice = deep.slice(text + 2, text + 6, true).expect("a slice");
    assert_eq!(slice.open_start(), levels + 2);
    let shallow = Node::from_json(&schema, &outer(inner(paragraph("x")))).expect("a document");
    let mut tr = Transform::new(shallow);
    tr.replace(0, 0, &slice).expect("replace");
    tr.doc().clone()
}

#[test]
fn deeply_open_slices_close_where_shallow_documents_take_them() {
    // prosemirror-transform 1.12.0's answer.
    let expected = r#"{"type":"doc","content":[{"type":"outer","content":[{"type":"inner","content":[{"type":"inner","content":[{"type":"inner","content":[{"type":"paragraph","content":[{"type":"text","text":"ep t"}]}]}]}]}]},{"type":"outer","content":[{"type":"inner","content":[{"type":"paragraph","content":[{"type":"text","text":"x"}]}]}]}]}"#;
    assert_eq!(fit_at_the_top(3).to_json_string(), expected);
    on_dirty_scheduler_stack(|| {
        let fitted = fit_at_the_top(FITTED);
        fitted.check().expect("a valid document");
        assert_eq!(fitted.child_count(), 2);
        assert_eq!(first_depth(&fitted), FITTED + 3);
        let text = fitted.text_content().expect("its text").to_string();
        assert_eq!(text, "ep tx");
    });
}

/// A paragraph wrapped in blockquotes far deeper than a small stack could recurse through, in
/// one step around it, which is applied, inverted, and applied again from its JSON.
#[test]
fn wrappers_nest_as_deeply_as_memory_allows() {
    on_dirty_scheduler_stack(|| {
        let schema = schema();
        let doc = shallow(&schema);
        let blockquote = schema.node_type("blockquote").expect("blockquote");
        let wrapper = Wrapper {
            node_type: blockquote,
            attrs: None,
        };
        let range = doc
            .resolve(1)
            .expect("a position")
            .block_range(&doc.resolve(2).expect("a position"), None)
            .expect("a range")
            .expect("a block range");
        let mut tr = Transform::new(doc.clone());
        tr.wrap(&range, &vec![wrapper; DEPTH]).expect("wrap");
        let wrapped = tr.doc().clone();
        assert_eq!(first_depth(&wrapped), DEPTH + 2);

        let [step] = tr.steps() else {
            panic!("one step");
        };
        // Inverting the inverse takes the blockquotes out from around the paragraph again.
        let inverted = step.invert(&doc).expect("its inverse");
        let undone = inverted.apply(wrapped.clone()).expect("applies");
        assert!(*undone.doc().expect("a document") == doc);
        let redone = inverted.invert(&wrapped).expect("the inverse's inverse");
        let redone = redone.apply(doc.clone()).expect("applies");
        assert!(*redone.doc().expect("a document") == wrapped);

        let steps = Value::Array(vec![step.to_json()]);
        assert!(api::apply_steps(&doc, &steps).expect("the step") == wrapped);
        let inverted = api::invert_steps(&doc, &steps).expect("its inverse");
        assert!(api::apply_steps(&wrapped, &inverted).expect("the inverse") == doc);
        let redone = api::invert_steps(&wrapped, &inverted).expect("the inverse's inverse");
        assert!(api::apply_steps(&doc, &redone).expect("the step again") == wrapped);
    });
}
