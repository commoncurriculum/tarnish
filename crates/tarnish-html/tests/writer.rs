//! The HTML the writer writes as the serializer goes is the HTML of the DOM the serializer
//! builds, whatever the specs: for random documents whose `toDOM`s give every shape of spec, the
//! two give the same HTML, or fail with the same error.

use std::collections::HashMap;

use tarnish::chunk::ValueRef;
use tarnish::dom::{
    AttrValue, DomSerializer, DomSpec, MarkToDom, NodeToDom, SpecAttrs, mark_to_dom, node_to_dom,
};
use tarnish::json::{Value, json};
use tarnish::{Node, Result, Schema, api, js};
use tarnish_html::{HtmlDom, HtmlNode, to_html};

type Spec<'a> = DomSpec<'a, HtmlNode>;

fn attrs<'a, const N: usize>(entries: [(&'a str, AttrValue<'a>); N]) -> SpecAttrs<'a> {
    SpecAttrs::from(entries)
}

fn text(text: &str) -> Spec<'_> {
    DomSpec::Text(text.into())
}

fn element<'a, const N: usize>(tag: &'a str, children: [Spec<'a>; N]) -> Spec<'a> {
    DomSpec::element(tag, SpecAttrs::new(), children.into())
}

/// A spec for content, chosen by `variant`, with `value` from the attributes in it.
fn block_spec(variant: u64, value: ValueRef<'_>) -> Spec<'_> {
    match variant {
        0 => DomSpec::wrapping("p", SpecAttrs::new()),
        1 => DomSpec::element(
            "div",
            attrs([
                ("class", "x".into()),
                ("style", "color: red;  background: BLUE; nonsense".into()),
                ("title", value.into()),
            ]),
            vec![DomSpec::Hole],
        ),
        2 => element(
            "section",
            [
                element("h2", [text("T<&>\u{a0}")]),
                DomSpec::element(
                    "div",
                    attrs([("data-a", value.into())]),
                    vec![DomSpec::Hole],
                ),
                element("hr", []),
                text("after"),
            ],
        ),
        3 => element("br", [DomSpec::Hole]),
        4 => element("template", [DomSpec::Hole]),
        5 => element("style", [DomSpec::Hole]),
        6 => DomSpec::wrapping("http://www.w3.org/2000/svg svg", SpecAttrs::new()),
        7 => DomSpec::wrapping("DIV", attrs([("Title", "t".into())])),
        8 => json!(["p", {"title": "x", "style": "margin: 0"}, 0]).into(),
        9 => element("div", [DomSpec::Hole, text("x")]),
        10 => element(
            "div",
            [element("a", [DomSpec::Hole]), element("b", [DomSpec::Hole])],
        ),
        11 => DomSpec::Attr(value),
        12 => DomSpec::element("div", attrs([("x y", "1".into())]), vec![DomSpec::Hole]),
        13 => DomSpec::element(
            "p",
            attrs([("hidden", value.into()), ("id", "a\"b".into())]),
            vec![DomSpec::Hole],
        ),
        14 => element("script", [text("<b>&</b>"), element("i", [DomSpec::Hole])]),
        15 => element("img", [text("inside"), DomSpec::Hole]),
        16 => element(
            "div",
            [element("template", [element("b", [DomSpec::Hole])])],
        ),
        17 => element(
            "div",
            [element("br", [text("x")]), element("span", [DomSpec::Hole])],
        ),
        18 => json!(["div", ["span", 0], ["em", "after"]]).into(),
        _ => element("div", [DomSpec::Attr(value), DomSpec::Hole]),
    }
}

fn leaf_spec(variant: u64, value: ValueRef<'_>) -> Spec<'_> {
    match variant % 6 {
        0 => DomSpec::Attr(value),
        1 => DomSpec::element("img", attrs([("src", value.into())]), vec![]),
        2 => element("hr", [DomSpec::Hole]),
        3 => element("span", [DomSpec::Attr(value)]),
        4 => json!(["span", {"class": "leaf"}, "text"]).into(),
        _ => text("leaf & text"),
    }
}

fn mark_spec(variant: u64, value: ValueRef<'_>) -> Spec<'_> {
    match variant % 9 {
        0 => DomSpec::wrapping("em", SpecAttrs::new()),
        1 => element("strong", []),
        2 => element("span", [element("b", [DomSpec::Hole])]),
        3 => text("mark text"),
        4 => json!(["code", 0]).into(),
        5 => element("br", []),
        6 => element("template", []),
        7 => DomSpec::element("a", attrs([("href", value.into())]), vec![]),
        _ => element("style", []),
    }
}

fn variant(attrs: ValueRef) -> u64 {
    attrs
        .get("variant")
        .and_then(ValueRef::as_f64)
        .unwrap_or(0.0) as u64
}

fn value(attrs: ValueRef) -> ValueRef {
    attrs.get("value").expect("a value")
}

fn schema() -> Schema {
    let spec = json!({
        "nodes": [
            ["doc", {"content": "block+"}],
            ["para", {"content": "inline*", "group": "block",
                      "attrs": {"variant": {"default": 0}, "value": {"default": null}}}],
            ["box", {"content": "block+", "group": "block",
                     "attrs": {"variant": {"default": 0}, "value": {"default": null}}}],
            ["leaf", {"group": "block",
                      "attrs": {"variant": {"default": 0}, "value": {"default": null}}}],
            ["text", {"group": "inline"}],
            ["icon", {"group": "inline", "inline": true,
                      "attrs": {"variant": {"default": 0}, "value": {"default": null}}}]
        ],
        "marks": [
            ["em", {"attrs": {"variant": {"default": 0}, "value": {"default": null}}}],
            ["strong", {"attrs": {"variant": {"default": 0}, "value": {"default": null}}}],
            ["code", {"spanning": false,
                      "attrs": {"variant": {"default": 0}, "value": {"default": null}}}]
        ]
    });
    api::schema(&spec).expect("a schema")
}

fn serializer() -> DomSerializer<HtmlNode> {
    let content: NodeToDom<HtmlNode> = node_to_dom(|node: &Node| {
        Ok(block_spec(
            variant(node.attrs_view()),
            value(node.attrs_view()),
        ))
    });
    let leaf: NodeToDom<HtmlNode> = node_to_dom(|node: &Node| {
        Ok(leaf_spec(
            variant(node.attrs_view()),
            value(node.attrs_view()),
        ))
    });
    let mark: MarkToDom<HtmlNode> = mark_to_dom(|mark, _| {
        Ok(mark_spec(
            variant(mark.attrs_view()),
            value(mark.attrs_view()),
        ))
    });
    let nodes = HashMap::from([
        ("para".to_owned(), content.clone()),
        ("box".to_owned(), content),
        ("leaf".to_owned(), leaf.clone()),
        ("icon".to_owned(), leaf),
    ]);
    let marks = HashMap::from([
        ("em".to_owned(), mark.clone()),
        ("strong".to_owned(), mark.clone()),
        ("code".to_owned(), mark),
    ]);
    DomSerializer::new(nodes, marks)
}

/// A xorshift generator, seeded so a failure reproduces.
struct Random(u64);

impl Random {
    fn below(&mut self, bound: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % bound
    }

    fn value(&mut self) -> Value {
        let values = [
            json!("str"),
            json!("<x> & \"y\""),
            json!(["p", 0]),
            json!(["div"]),
            json!([1, 2]),
            json!({"0": "p", "length": 1}),
            json!(0),
            json!(1.5),
            json!(null),
            json!(true),
        ];
        values[self.below(values.len() as u64) as usize].clone()
    }

    fn attrs(&mut self, variants: u64) -> Value {
        json!({"variant": self.below(variants), "value": self.value()})
    }

    fn marks(&mut self) -> Value {
        let mut marks = Vec::new();
        for name in ["em", "strong", "code"] {
            if self.below(3) == 0 {
                marks.push(json!({"type": name, "attrs": self.attrs(9)}));
            }
        }
        Value::Array(marks)
    }

    fn inline(&mut self) -> Value {
        let mut content = Vec::new();
        for _ in 0..self.below(4) {
            content.push(match self.below(4) {
                0 => json!({"type": "icon", "attrs": self.attrs(6), "marks": self.marks()}),
                _ => {
                    let texts = ["a", "<b> & c", "\u{a0}", "x y"];
                    let text = texts[self.below(texts.len() as u64) as usize];
                    json!({"type": "text", "text": text, "marks": self.marks()})
                }
            });
        }
        Value::Array(content)
    }

    // bounded: past depth 2 a block holds no blocks
    fn block(&mut self, depth: u32) -> Value {
        match self.below(if depth > 2 { 2 } else { 3 }) {
            0 => json!({"type": "para", "attrs": self.attrs(20), "content": self.inline()}),
            1 => json!({"type": "leaf", "attrs": self.attrs(6)}),
            _ => {
                let content: Vec<Value> =
                    (0..=self.below(2)).map(|_| self.block(depth + 1)).collect();
                json!({"type": "box", "attrs": self.attrs(20), "content": content})
            }
        }
    }
}

fn through_dom(serializer: &DomSerializer<HtmlNode>, doc: &Node<'static>) -> Result<String> {
    let dom = HtmlDom::new();
    Ok(serializer
        .serialize_fragment(&dom, doc.content(), None)?
        .inner_html())
}

fn outcome(result: Result<String>) -> std::result::Result<String, String> {
    result.map_err(|error| error.to_string())
}

#[test]
fn writes_the_html_of_the_dom_it_would_build() {
    let schema = schema();
    let serializer = serializer();
    let mut random = Random(0x9E37_79B9_7F4A_7C15);
    let (mut written, mut failed) = (0, 0);
    for _ in 0..20_000 {
        let content: Vec<Value> = (0..=random.below(3)).map(|_| random.block(0)).collect();
        let json = json!({"type": "doc", "content": content});
        let doc = Node::from_json(&schema, &json).expect("a document");
        let expected = outcome(through_dom(&serializer, &doc));
        let actual = outcome(to_html(&serializer, doc.content()));
        assert_eq!(actual, expected, "{}", js::json::stringify(&json));
        match expected {
            Ok(_) => written += 1,
            Err(_) => failed += 1,
        }
    }
    // Both kinds of outcome are common, so neither is compared only rarely.
    assert!(
        written > 2_000 && failed > 2_000,
        "{written} written, {failed} failed"
    );
}
