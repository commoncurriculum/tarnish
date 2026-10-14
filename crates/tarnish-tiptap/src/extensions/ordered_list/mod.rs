//! `@tiptap/extension-list`'s `OrderedList`, which `@tiptap/extension-ordered-list` exports, with
//! its list markers of numbers, letters and roman numerals (`roman.ts`) and its Markdown
//! tokenizer (`utils.ts`).

pub(super) mod roman;
mod utils;

use std::sync::LazyLock;

use super::bullet_list::render_list;
use crate::markdown::{ParseHelpers, Parsed};
use crate::merge_attributes::merge_html_attributes;
use crate::{AttrValue, DomSpec, ExtensionAttribute, HtmlNode, NodeExtension, ParseHtml};
use tarnish_js::json::{Map, Value, json};
use tarnish_js::regexp::RegExp;
use tarnish_js::{self as js, Error, utf16};
use tarnish_markdown::marked::Token;
use utils::{parse_list_items, tokenize};

pub struct OrderedListOptions {
    /// The node type of the list's items.
    pub item_type_name: &'static str,
    pub html_attributes: Map,
}

impl Default for OrderedListOptions {
    fn default() -> Self {
        OrderedListOptions {
            item_type_name: "listItem",
            html_attributes: Map::new(),
        }
    }
}

pub fn ordered_list(options: OrderedListOptions) -> NodeExtension {
    let attributes = options.html_attributes;
    NodeExtension::create("orderedList")
        .group("block list")
        .content(format!("{}+", options.item_type_name))
        .add_attributes(vec![
            ExtensionAttribute::new("start", json!(1)).parse_html(|element| {
                Some(match element.attribute("start") {
                    Some(start) => js::number(js::parse_int_radix(&start, 10)),
                    None => json!(1),
                })
            }),
            ExtensionAttribute::new("type", Value::Null).parse_html(html_type),
        ])
        .parse_html([ParseHtml::tag("ol")])
        .render_html(move |_, mut html| {
            let start = html.remove("start");
            let kind = html.remove("type");
            let mut attrs = merge_html_attributes(&attributes, html)?;
            if let Some(start) = start.filter(|start| !is_one(start)) {
                attrs.set("start", start);
            }
            if let Some(kind) = kind.filter(|kind| kind.truthy() && kind.as_str() != Some("1")) {
                attrs.set("type", kind);
            }
            Ok(DomSpec::wrapping("ol", attrs))
        })
        .markdown_token_name("list")
        .parse_markdown(parse_markdown)
        .render_markdown(render_list)
        // A marker the start would find past a paragraph's first unit is mid-line, and marked
        // already ends a paragraph before a list that starts a line.
        .markdown_block_tokenizer(|_| None, tokenize)
        .indents_content()
}

/// `start === 1`: the number, which a string isn't.
fn is_one(start: &AttrValue) -> bool {
    match start {
        AttrValue::Json(start) => start.as_f64() == Some(1.0),
        AttrValue::Owned(start) => start.as_f64() == Some(1.0),
        AttrValue::Text(_) => false,
    }
}

static LIST_STYLE_TYPE: LazyLock<RegExp> =
    LazyLock::new(|| RegExp::new(r"list-style-type\s*:\s*([^;]+)", "i"));

/// The `type` attribute's `parseHTML`: the list's `type`, or the HTML type its CSS
/// `list-style-type` maps to, or its first item's.
fn html_type(element: &HtmlNode) -> Option<Value> {
    if let Some(kind) = element.attribute("type").filter(|kind| !kind.is_empty()) {
        return Some(Value::String(kind));
    }
    let mapped = |element: &HtmlNode| {
        let style = element
            .attribute("style")
            .filter(|style| !style.is_empty())?;
        css_list_style_type_to_html_type(&style)
    };
    if let Some(kind) = mapped(element) {
        return Some(json!(kind));
    }
    let first_item = element.query_selector("li").ok().flatten()?;
    mapped(&first_item).map(|kind| json!(kind))
}

/// `cssListStyleTypeToHtmlType(style)`.
fn css_list_style_type_to_html_type(style: &str) -> Option<&'static str> {
    let style = utf16::from(style);
    let found = LIST_STYLE_TYPE.exec(&style)?;
    let value = utf16::to_string(utf16::trim(found.get(1).unwrap_or_default())).to_lowercase();
    match value.as_str() {
        "upper-roman" => Some("I"),
        "lower-roman" => Some("i"),
        "upper-alpha" | "upper-latin" => Some("A"),
        "lower-alpha" | "lower-latin" => Some("a"),
        _ => None,
    }
}

fn parse_markdown(token: &Token, helpers: &ParseHelpers) -> Result<Parsed, Error> {
    let Some(list) = token
        .list()
        .filter(|list| token.kind == "list" && list.ordered)
    else {
        return Ok(Parsed::Nodes(Vec::new()));
    };
    // `token.start || 1`
    let start = list
        .start
        .filter(|start| js::number_to_boolean(*start))
        .unwrap_or(1.0);
    let content = parse_list_items(&list.items, helpers)?;
    let mut attrs = Map::new();
    if start != 1.0 {
        attrs.insert("start".into(), js::number(start));
    }
    if let Some(kind) = token
        .extra("typeMarker")
        .filter(|kind| js::truthy(Some(kind)))
    {
        attrs.insert("type".into(), kind.clone());
    }
    Ok(Parsed::Node(if attrs.is_empty() {
        json!({ "type": "orderedList", "content": content })
    } else {
        json!({ "type": "orderedList", "attrs": attrs, "content": content })
    }))
}
