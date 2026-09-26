//! Transforms the upstream suites don't reach, each expected to give or throw what
//! prosemirror-transform 1.12.0 gives or throws.

use tarnish::js::json::stringify;
use tarnish::json;
use tarnish::transform::{Step, Transform, Wrapper, can_split, drop_point, replace_step};
use tarnish::{Fragment, Node, Schema, Slice, Value, api};

/// A figure's title can't be made up, having an attribute with no default, so nothing fills
/// the start of a figure before its paragraph.
fn schema() -> Schema {
    let spec = r#"{"nodes": [
        ["doc", {"content": "block+"}],
        ["paragraph", {"content": "inline*", "group": "block"}],
        ["figure", {"content": "(title paragraph) | caption", "group": "block"}],
        ["title", {"content": "inline*", "attrs": {"level": {}}}],
        ["caption", {"content": "inline*"}],
        ["text", {"group": "inline"}]
    ]}"#;
    api::schema(&json::from_str(spec).expect("the spec")).expect("the schema")
}

/// `<p>a</p><figure><title>t</title><p>cd</p></figure>`, where 3 is between the blocks and 9 is
/// between "c" and "d".
fn doc(schema: &Schema) -> Node {
    let json = r#"{"type": "doc", "content": [
        {"type": "paragraph", "content": [{"type": "text", "text": "a"}]},
        {"type": "figure", "content": [
            {"type": "title", "attrs": {"level": 1}, "content": [{"type": "text", "text": "t"}]},
            {"type": "paragraph", "content": [{"type": "text", "text": "cd"}]}
        ]}
    ]}"#;
    Node::from_json(schema, &json::from_str(json).expect("JSON")).expect("the document")
}

#[test]
fn a_fit_opens_a_node_nothing_fills_empty() {
    let schema = schema();
    let step = replace_step(&doc(&schema), 3, 9, &Slice::empty())
        .expect("a fit")
        .expect("a step");
    assert_eq!(
        stringify(&step.to_json()),
        r#"{"stepType":"replace","from":3,"to":9,"slice":{"content":[{"type":"figure","content":[{"type":"paragraph"}]}],"openEnd":2}}"#
    );
}

#[test]
fn reading_what_isnt_there_throws_javascripts_type_error() {
    let schema = schema();
    let doc = doc(&schema);
    let end = doc.content().size();
    let paragraph = schema.node_type("paragraph").expect("paragraph");
    let text = schema.text("ab", &[]).expect("text");
    let ab = paragraph
        .create(None, Fragment::from_node(text), &[])
        .expect("a paragraph");
    // Open three levels deep around a paragraph, which has two.
    let deep = Slice::new(Fragment::from_node(ab), 3, 0);
    let attr = Step::Attr {
        pos: end,
        attr: "level".into(),
        value: Some(Value::from(2)),
    };
    let null = |property: &str| {
        format!("TypeError: Cannot read properties of null (reading '{property}')")
    };
    let cases = [
        (
            "canSplit(doc, 2, 0)",
            can_split(&doc, 2, 0, &[]).map(drop),
            "TypeError: Cannot read properties of undefined (reading 'type')".to_string(),
        ),
        (
            "tr.clearIncompatible(end, paragraph)",
            Transform::new(doc.clone())
                .clear_incompatible(end, &paragraph, None)
                .map(drop),
            null("childCount"),
        ),
        (
            "new AttrStep(end, 'level', 2).invert(doc)",
            attr.invert(&doc).map(drop),
            null("attrs"),
        ),
        (
            "replaceStep(doc, 1, 1, deep)",
            replace_step(&doc, 1, 1, &deep).map(drop),
            null("type"),
        ),
        (
            "dropPoint(doc, 1, deep)",
            drop_point(&doc, 1, &deep).map(drop),
            null("content"),
        ),
        (
            "tr.replaceRange(1, 1, deep)",
            Transform::new(doc.clone())
                .replace_range(1, 1, &deep)
                .map(drop),
            null("content"),
        ),
    ];
    for (call, result, thrown) in cases {
        assert_eq!(result.expect_err(call).to_string(), thrown, "{call}");
    }
    let wrapper = Wrapper {
        node_type: paragraph,
        attrs: None,
    };
    assert!(!can_split(&doc, 2, 0, &[Some(wrapper)]).expect("an answer"));
}

#[test]
fn joining_before_the_start_is_out_of_range() {
    let error = Transform::new(doc(&schema()))
        .join(0, 1)
        .expect_err("a RangeError");
    assert_eq!(error.to_string(), "RangeError: Position -1 out of range");
}

#[test]
fn splitting_past_the_top_fails_the_step() {
    let error = Transform::new(doc(&schema()))
        .split(2, 2, &[])
        .expect_err("a TransformError");
    assert_eq!(
        error.to_string(),
        "TransformError: Inserted content deeper than insertion position"
    );
}
