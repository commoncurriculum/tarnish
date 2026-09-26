//! Transforms the upstream suites don't reach, each expected to give what prosemirror-transform
//! 1.12.0 gives.

use tarnish::js::json::stringify;
use tarnish::json;
use tarnish::transform::{Wrapper, can_split, replace_step};
use tarnish::{Node, Schema, Slice, api};

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
fn splitting_no_levels_reads_the_type_of_a_node_that_isnt_there() {
    let schema = schema();
    let doc = doc(&schema);
    let error = can_split(&doc, 2, 0, None).expect_err("a TypeError");
    assert_eq!(error.class(), "Error");
    let paragraph = Wrapper {
        node_type: schema.node_type("paragraph").expect("paragraph"),
        attrs: None,
    };
    assert!(!can_split(&doc, 2, 0, Some(&[Some(paragraph)])).expect("an answer"));
}
