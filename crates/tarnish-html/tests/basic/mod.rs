//! prosemirror-schema-basic 1.2.4's and prosemirror-schema-list 1.5.1's `parseDOM` rules and
//! `toDOM` functions, as their `dist/index.js` gives them, over this crate's DOM.

use std::collections::HashMap;
use std::sync::Arc;

use tarnish::dom::{
    ClearMarkHook, DomParser, DomSerializer, DomSpec, GetAttrsResult, MarkToDom, NodeToDom,
    ParseRule, PreserveWhitespace, Rule, StyleRule, TagRule,
};
use tarnish::json::{self, Map, Value};
use tarnish::{Node, Result, Schema, js};
use tarnish_html::HtmlNode;

fn tag(selector: &str) -> ParseRule<HtmlNode> {
    ParseRule::Tag(Rule::new(TagRule::new(selector)))
}

fn tag_with_attrs(
    selector: &str,
    get_attrs: impl Fn(&HtmlNode) -> Result<GetAttrsResult> + Send + Sync + 'static,
) -> ParseRule<HtmlNode> {
    let mut rule = TagRule::new(selector);
    rule.get_attrs = Some(Arc::new(get_attrs));
    ParseRule::Tag(Rule::new(rule))
}

fn style(style: &str) -> ParseRule<HtmlNode> {
    ParseRule::Style(Rule::new(StyleRule::new(style)))
}

/// A style rule that takes marks of this type off the content.
fn clearing(style: &str, mark_type: &'static str) -> ParseRule<HtmlNode> {
    let mut rule = StyleRule::new(style);
    let clear: ClearMarkHook = Arc::new(move |mark| Ok(mark.mark_type().name() == mark_type));
    rule.clear_mark = Some(clear);
    ParseRule::Style(Rule::new(rule))
}

/// `dom.getAttribute(name)`, `null` when there is none.
fn attribute(dom: &HtmlNode, name: &str) -> Value {
    dom.attribute(name).map_or(Value::Null, Value::String)
}

fn attrs(entries: impl IntoIterator<Item = (&'static str, Value)>) -> GetAttrsResult {
    let entries = entries.into_iter().map(|(key, value)| (key.into(), value));
    GetAttrsResult::Attrs(Map::from_iter(entries))
}

/// `/^(bold(er)?|[5-9]\d{2,})$/.test(value)`.
fn is_bold(value: &str) -> bool {
    let heavy = value.len() >= 3
        && value.starts_with(|c: char| ('5'..='9').contains(&c))
        && value.bytes().all(|byte| byte.is_ascii_digit());
    value == "bold" || value == "bolder" || heavy
}

/// JavaScript's `+string`.
fn to_number(text: &str) -> f64 {
    let is_space = |c: char| {
        matches!(
            c,
            '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
                ..='\u{200a}'
                    | '\u{2028}'
                    | '\u{2029}'
                    | '\u{202f}'
                    | '\u{205f}'
                    | '\u{3000}'
                    | '\u{feff}'
        )
    };
    let text = text.trim_matches(is_space);
    if text.is_empty() {
        return 0.0;
    }
    let radix = |digits: &str, radix: u32| {
        let value = digits.chars().try_fold(0.0, |value: f64, digit| {
            digit
                .to_digit(radix)
                .map(|digit| value * f64::from(radix) + f64::from(digit))
        });
        value.filter(|_| !digits.is_empty()).unwrap_or(f64::NAN)
    };
    for (prefixes, base) in [(["0x", "0X"], 16), (["0o", "0O"], 8), (["0b", "0B"], 2)] {
        if let Some(digits) = prefixes.iter().find_map(|prefix| text.strip_prefix(prefix)) {
            return radix(digits, base);
        }
    }
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    if unsigned == "Infinity" {
        return if text.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(at) => (&unsigned[..at], Some(&unsigned[at + 1..])),
        None => (unsigned, None),
    };
    let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let valid_mantissa =
        digits(whole) && digits(fraction) && !(whole.is_empty() && fraction.is_empty());
    let valid_exponent = exponent.is_none_or(|exponent| {
        let exponent = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        !exponent.is_empty() && digits(exponent)
    });
    match valid_mantissa && valid_exponent {
        true => text.parse().unwrap_or(f64::NAN),
        false => f64::NAN,
    }
}

fn node_rules() -> HashMap<&'static str, Vec<ParseRule<HtmlNode>>> {
    let mut code_block = TagRule::new("pre");
    code_block.element.preserve_whitespace = Some(PreserveWhitespace::Full);
    let headings = (1..=6).map(|level| {
        let mut rule = Rule::new(TagRule::new(format!("h{level}")));
        rule.attrs = Some(json::object!({"level": level}));
        ParseRule::Tag(rule)
    });
    HashMap::from([
        ("paragraph", vec![tag("p")]),
        ("blockquote", vec![tag("blockquote")]),
        ("horizontal_rule", vec![tag("hr")]),
        ("heading", headings.collect()),
        ("code_block", vec![ParseRule::Tag(Rule::new(code_block))]),
        (
            "image",
            vec![tag_with_attrs("img[src]", |dom| {
                Ok(attrs([
                    ("src", attribute(dom, "src")),
                    ("title", attribute(dom, "title")),
                    ("alt", attribute(dom, "alt")),
                ]))
            })],
        ),
        ("hard_break", vec![tag("br")]),
        (
            "ordered_list",
            vec![tag_with_attrs("ol", |dom| {
                let order = match dom.has_attribute("start") {
                    true => js::number(to_number(&dom.attribute("start").unwrap_or_default())),
                    false => Value::from(1),
                };
                Ok(attrs([("order", order)]))
            })],
        ),
        ("bullet_list", vec![tag("ul")]),
        ("list_item", vec![tag("li")]),
    ])
}

fn mark_rules() -> HashMap<&'static str, Vec<ParseRule<HtmlNode>>> {
    HashMap::from([
        (
            "link",
            vec![tag_with_attrs("a[href]", |dom| {
                Ok(attrs([
                    ("href", attribute(dom, "href")),
                    ("title", attribute(dom, "title")),
                ]))
            })],
        ),
        (
            "em",
            vec![
                tag("i"),
                tag("em"),
                style("font-style=italic"),
                clearing("font-style=normal", "em"),
            ],
        ),
        (
            "strong",
            vec![
                tag("strong"),
                tag_with_attrs("b", |node| {
                    Ok(match node.style_value("font-weight") != "normal" {
                        true => GetAttrsResult::Defaults,
                        false => GetAttrsResult::Reject,
                    })
                }),
                clearing("font-weight=400", "strong"),
                {
                    let mut rule = StyleRule::new("font-weight");
                    rule.get_attrs = Some(Arc::new(|value| {
                        Ok(match is_bold(value) {
                            true => GetAttrsResult::Defaults,
                            false => GetAttrsResult::Reject,
                        })
                    }));
                    ParseRule::Style(Rule::new(rule))
                },
            ],
        ),
        ("code", vec![tag("code")]),
    ])
}

/// `DOMParser.fromSchema(schema)`.
pub fn parser(schema: &Schema) -> DomParser<HtmlNode> {
    let (mut marks, mut nodes) = (mark_rules(), node_rules());
    let marks = schema
        .mark_types()
        .map(|mark_type| marks.remove(mark_type.name()));
    let nodes = schema
        .node_types()
        .map(|node_type| nodes.remove(node_type.name()));
    let marks = marks.map(Option::unwrap_or_default).collect();
    let nodes = nodes.map(Option::unwrap_or_default).collect();
    DomParser::from_schema(schema.clone(), marks, nodes).expect("a parser")
}

fn attr(node: &Node<'static>, name: &str) -> Value {
    node.attrs()
        .get(name)
        .map_or(Value::Null, |value| value.to_value())
}

fn node_spec(
    to_dom: impl Fn(&Node<'static>) -> Value + Send + Sync + 'static,
) -> NodeToDom<HtmlNode> {
    Arc::new(move |node| Ok(DomSpec::from(to_dom(node))))
}

fn mark_spec(spec: Value) -> MarkToDom<HtmlNode> {
    Arc::new(move |_, _| Ok(DomSpec::from(spec.clone())))
}

/// `DOMSerializer.fromSchema(schema)`.
pub fn serializer() -> DomSerializer<HtmlNode> {
    let nodes = HashMap::from([
        ("paragraph", node_spec(|_| json::json!(["p", 0]))),
        ("blockquote", node_spec(|_| json::json!(["blockquote", 0]))),
        ("horizontal_rule", node_spec(|_| json::json!(["hr"]))),
        (
            "heading",
            node_spec(|node| {
                let level = js::to_string(&attr(node, "level")).expect("a level converts");
                let tag = format!("h{level}");
                json::json!([tag, 0])
            }),
        ),
        (
            "code_block",
            node_spec(|_| json::json!(["pre", ["code", 0]])),
        ),
        (
            "image",
            node_spec(|node| {
                let (src, alt, title) = (attr(node, "src"), attr(node, "alt"), attr(node, "title"));
                json::json!(["img", {"src": src, "alt": alt, "title": title}])
            }),
        ),
        ("hard_break", node_spec(|_| json::json!(["br"]))),
        (
            "ordered_list",
            node_spec(|node| {
                let order = attr(node, "order");
                // `node.attrs.order == 1`, which a number is when it is one.
                match order.as_f64() == Some(1.0) {
                    true => json::json!(["ol", 0]),
                    false => json::json!(["ol", {"start": order}, 0]),
                }
            }),
        ),
        ("bullet_list", node_spec(|_| json::json!(["ul", 0]))),
        ("list_item", node_spec(|_| json::json!(["li", 0]))),
    ]);
    let link: MarkToDom<HtmlNode> = Arc::new(|mark, _| {
        let attrs = mark.attrs();
        let get = |name| {
            attrs
                .get(name)
                .map_or(Value::Null, |value| value.to_value())
        };
        let (href, title) = (get("href"), get("title"));
        Ok(DomSpec::from(
            json::json!(["a", {"href": href, "title": title}, 0]),
        ))
    });
    let marks = HashMap::from([
        ("link", link),
        ("em", mark_spec(json::json!(["em", 0]))),
        ("strong", mark_spec(json::json!(["strong", 0]))),
        ("code", mark_spec(json::json!(["code", 0]))),
    ]);
    DomSerializer::new(
        nodes
            .into_iter()
            .map(|(name, to_dom)| (name.to_owned(), to_dom))
            .collect(),
        marks
            .into_iter()
            .map(|(name, to_dom)| (name.to_owned(), to_dom))
            .collect(),
    )
}
