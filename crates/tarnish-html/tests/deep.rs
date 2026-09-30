//! HTML and specs nested far deeper than a small stack could recurse through, parsed, rendered
//! and written on a thread with such a stack.

mod basic;

use tarnish::dom::{DomSpec, ParseOptions, render_spec, render_spec_of};
use tarnish::js::stack::on_dirty_scheduler_stack;
use tarnish::json::{self, Value};
use tarnish::{Error, Node, Schema, api};
use tarnish_html::{HtmlDom, HtmlNode, parse_html, to_html};

const DEPTH: usize = 3_000;

/// How deep specs and attributes nest, which is deeper than HTML, whose tree builder takes time
/// that grows with the square of the depth.
const SPEC_DEPTH: usize = 20_000;

#[test]
fn deep_html_round_trips_on_a_small_stack() {
    on_dirty_scheduler_stack(|| {
        let html = format!(
            "{}<p>deep</p>{}",
            "<blockquote>".repeat(DEPTH),
            "</blockquote>".repeat(DEPTH)
        );
        let fixtures = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/dom.json"
        ))
        .expect("the fixtures");
        let fixtures = json::from_str(&fixtures).expect("JSON");
        let schema = api::schema(&fixtures["schema"]).expect("the schema");
        let parser = basic::parser(&schema);

        let fragment = HtmlDom::new().parse_fragment(&html);
        assert_eq!(fragment.inner_html(), html);

        let doc =
            parse_html(&parser, &html, ParseOptions::<HtmlNode>::default()).expect("a document");
        let mut depth = 0;
        let mut node = doc.clone();
        while let Some(child) = node.first_child() {
            depth += 1;
            node = child;
        }
        assert_eq!(depth, DEPTH + 2);
        assert_eq!(
            to_html(&basic::serializer(), doc.content()).expect("HTML"),
            html
        );
    });
}

/// A schema whose `holder` holds any JSON in its attributes, as specs may come from them.
fn holders() -> Schema {
    let spec = json::json!({"nodes": [
        ["doc", {"content": "holder*"}],
        ["holder", {"attrs": {"deep": {"default": null}, "spec": {}}}],
        ["text", {}],
    ]});
    api::schema(&spec).expect("the schema")
}

fn holder(schema: &Schema, attrs: Value) -> Node<'static> {
    let holder = json::json!({"type": "holder", "attrs": attrs});
    Node::from_json(schema, &holder).expect("a holder")
}

/// `<span>` inside `<span>`, `SPEC_DEPTH` deep, around `x`: as an array, or as an object that
/// `renderSpec` reads by its indices and `length`, whose second item, an object, is attributes.
fn nested_spec(array_like: bool) -> Value {
    (0..SPEC_DEPTH).fold(Value::String("x".into()), |inner, _| match array_like {
        true => json::json!({"0": "span", "1": {}, "2": inner, "length": 3}),
        false => json::json!(["span", inner]),
    })
}

fn rendered(spec: &DomSpec<HtmlNode>) -> Result<String, Error> {
    render_spec(&HtmlDom::new(), spec, None).map(|rendered| rendered.dom.outer_html())
}

#[test]
fn specs_render_as_deeply_as_memory_allows() {
    on_dirty_scheduler_stack(|| {
        let html = "<span>".repeat(SPEC_DEPTH) + "x" + &"</span>".repeat(SPEC_DEPTH);
        let schema = holders();
        let array = holder(&schema, json::json!({"spec": nested_spec(false)}));
        let array_like = holder(&schema, json::json!({"spec": nested_spec(true)}));
        let attribute = |holder: &Node<'static>| {
            let spec = holder.attrs_view().get("spec").expect("its spec");
            rendered(&DomSpec::Attr(spec)).expect("HTML")
        };
        assert_eq!(
            rendered(&DomSpec::from(nested_spec(false))).expect("HTML"),
            html
        );
        assert_eq!(attribute(&array), html);
        assert_eq!(attribute(&array_like), html);
    });
}

/// Before rendering an array from a node's attributes as a spec, `renderSpec` looks for it
/// through every array and object they hold.
#[test]
fn looks_for_a_spec_through_attributes_nested_as_deeply_as_memory_allows() {
    on_dirty_scheduler_stack(|| {
        let deep = (0..SPEC_DEPTH).fold(Value::Null, |inner, level| match level % 2 {
            0 => json::json!([inner]),
            _ => json::json!({"a": inner}),
        });
        let schema = holders();
        let holder = holder(&schema, json::json!({"deep": deep, "spec": ["span"]}));
        let attrs = holder.attrs_view();
        let spec = DomSpec::Attr(attrs.get("spec").expect("its spec"));
        let refused = render_spec_of(&HtmlDom::new(), &spec, attrs).err();
        let message = refused.as_ref().map(Error::message);
        assert!(message.is_some_and(|message| message.contains("cross site scripting")));
    });
}
