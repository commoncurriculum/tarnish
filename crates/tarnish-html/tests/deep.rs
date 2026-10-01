//! HTML, specs and styles nested far deeper than a small stack could recurse through, parsed,
//! rendered and written on a thread with such a stack.

mod basic;

use std::collections::HashMap;
use std::sync::Arc;

use tarnish::dom::{DomSerializer, DomSpec, NodeToDom, ParseOptions, render_spec, render_spec_of};
use tarnish::js::stack::on_dirty_scheduler_stack;
use tarnish::json::{self, Value};
use tarnish::{Error, Node, Schema, api};
use tarnish_fixtures::read;
use tarnish_html::{HtmlDom, HtmlNode, parse_html, to_html};

const DEPTH: usize = 3_000;

/// How deep specs and attributes nest, which is deeper than HTML, whose tree builder takes time
/// that grows with the square of the depth.
const SPEC_DEPTH: usize = 20_000;

/// How deep blocks nest in a style's CSS, which the CSS engine reads in time that grows with
/// the depth.
const CSS_DEPTH: usize = 100_000;

/// How deep selector lists nest in each other.
const SELECTOR_DEPTH: usize = 10_000;

#[test]
fn deep_html_round_trips_on_a_small_stack() {
    on_dirty_scheduler_stack(|| {
        let html = format!(
            "{}<p>deep</p>{}",
            "<blockquote>".repeat(DEPTH),
            "</blockquote>".repeat(DEPTH)
        );
        let schema = api::schema(&read("dom")["schema"]).expect("the schema");
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

/// Styles whose CSS nests far deeper than the HTML does: the CSS engine reads each element's
/// style as the parser walks it, and writes a style it's given as the attribute.
#[test]
fn styles_nest_as_deeply_as_memory_allows() {
    on_dirty_scheduler_stack(|| {
        let parens = "(".repeat(CSS_DEPTH) + &")".repeat(CSS_DEPTH);
        let schema = api::schema(&read("dom")["schema"]).expect("the schema");
        let parser = basic::parser(&schema);
        // A style as given, and as the CSS engine writes it back.
        let styles = [
            (
                format!("--a:{}", "(".repeat(CSS_DEPTH)),
                format!("--a: {parens};"),
            ),
            (
                format!("color: var(--a,{parens})"),
                format!("color: var(--a,{parens});"),
            ),
        ];
        for (given, kept) in styles {
            let html = format!("<p style=\"{given}\">x</p>");
            let doc = parse_html(&parser, &html, ParseOptions::<HtmlNode>::default());
            let doc = doc.expect("a document");
            assert_eq!(
                to_html(&basic::serializer(), doc.content()).expect("HTML"),
                "<p>x</p>"
            );

            let styled = format!("<p style=\"{kept}\">x</p>");
            let spec = json::json!(["p", {"style": given.clone()}, "x"]);
            assert!(rendered(&DomSpec::from(spec)).expect("HTML") == styled);

            let paragraph: NodeToDom<HtmlNode> =
                Arc::new(move |_| Ok(json::json!(["p", {"style": given.clone()}, 0]).into()));
            let serializer = DomSerializer::new(
                HashMap::from([("paragraph".to_owned(), paragraph)]),
                HashMap::new(),
            );
            assert!(to_html(&serializer, doc.content()).expect("HTML") == styled);
        }
    });
}

/// Selectors that nest, or chain combinators, far deeper than a small stack could recurse
/// through, matched in HTML nested as deeply as they chain, and `:has()` looking down through it.
#[test]
fn selectors_nest_as_deeply_as_memory_allows() {
    on_dirty_scheduler_stack(|| {
        let fragment = HtmlDom::new().parse_fragment(&("<div>".repeat(DEPTH) + "<p>x</p>"));
        let found =
            |node: &HtmlNode, selector: &str| node.query_selector(selector).expect("a selector");
        let paragraph = found(&fragment, "p").expect("the paragraph");
        let top = found(&fragment, "div").expect("the top div");

        let nested = |open: &str, close: &str| {
            open.repeat(SELECTOR_DEPTH) + "p" + &close.repeat(SELECTOR_DEPTH)
        };
        let selectors = [
            nested(":is(", ")"),
            nested(":not(:not(", "))"),
            "div ".repeat(DEPTH) + "p",
            "div > ".repeat(DEPTH) + "p",
        ];
        for selector in selectors {
            assert!(found(&fragment, &selector).as_ref() == Some(&paragraph));
            let closest = paragraph.closest(&selector).expect("a selector");
            assert!(closest.as_ref() == Some(&paragraph));
        }
        assert!(found(&fragment, "div:has(p)").as_ref() == Some(&top));
        assert!(top.closest("div:has(p)").expect("a selector").as_ref() == Some(&top));
    });
}
