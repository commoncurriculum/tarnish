//! An HTML DOM for tarnish's [`DomParser`] and [`DomSerializer`]: parse HTML into ProseMirror
//! documents, and write documents as HTML, as ProseMirror does in a DOM that follows the
//! standards.
//!
//! [`HtmlDom`] parses HTML with html5gum's tokenizer and html5ever's tree builder, which follow
//! the HTML standard's parsing algorithm, and writes it as the standard's `innerHTML` and
//! `outerHTML` do. Elements match CSS selectors with servo's `selectors`, and hold their inline
//! style in tarnish-css.
//!
//! [`parse_html`] parses with a DOM of its own. To parse a whole document, or hand a parser a
//! node, make the DOM with [`HtmlDom::parse_document`] or [`HtmlDom::parse_fragment`] and pass
//! it as the parser's [`Dom`](tarnish::dom::Dom). [`to_html`] writes the HTML the serializer
//! would build in a DOM as it goes, without building it; [`HtmlWriter`] is the
//! [`Target`](tarnish::dom::Target) it writes through.
//!
//! [`names`] and [`serialize`] give the DOM's name checks and serialization's rules as
//! functions of strings, for a DOM of another shape to share.
//!
//! ```
//! use std::collections::HashMap;
//! use std::sync::Arc;
//!
//! use tarnish::dom::{
//!     DomParser, DomSerializer, DomSpec, GetAttrsResult, MarkToDom, NodeToDom, ParseOptions,
//!     ParseRule, Rule, TagRule,
//! };
//! use tarnish::{api, json};
//! use tarnish_html::{HtmlNode, parse_html, to_html};
//!
//! let spec = r#"{"nodes": {"doc": {"content": "paragraph+"},
//!                          "paragraph": {"content": "text*"}, "text": {}},
//!                "marks": {"link": {"attrs": {"href": {}}}}}"#;
//! let schema = api::schema(&json::from_str(spec).expect("JSON"))?;
//!
//! // Each type's parse rules, in schema order: the marks', then the nodes'.
//! let mut link = TagRule::new("a[href]");
//! link.get_attrs = Some(Arc::new(|element: &HtmlNode| {
//!     let href = element.attribute("href").unwrap_or_default();
//!     Ok(GetAttrsResult::Attrs(json::object!({"href": href})))
//! }));
//! let marks = vec![vec![ParseRule::Tag(Rule::new(link))]];
//! let nodes = vec![vec![], vec![ParseRule::Tag(Rule::new(TagRule::new("p")))], vec![]];
//! let parser = DomParser::from_schema(schema, marks, nodes)?;
//!
//! let html = "<p>Read <a href='/docs'>the docs</a>.</p>";
//! let doc = parse_html(&parser, html, ParseOptions::default())?;
//!
//! let paragraph: NodeToDom<HtmlNode> = Arc::new(|_| Ok(json::json!(["p", 0]).into()));
//! let link: MarkToDom<HtmlNode> = Arc::new(|mark, _| {
//!     let href = mark.attrs().to_map().remove("href").unwrap_or_default();
//!     Ok(DomSpec::from(json::json!(["a", {"href": href}, 0])))
//! });
//! let serializer = DomSerializer::new(
//!     HashMap::from([("paragraph".to_owned(), paragraph)]),
//!     HashMap::from([("link".to_owned(), link)]),
//! );
//! let html = to_html(&serializer, doc.content())?;
//! assert_eq!(html, r#"<p>Read <a href="/docs">the docs</a>.</p>"#);
//! # Ok::<(), tarnish::Error>(())
//! ```

#![forbid(unsafe_code)]

mod dom;
mod element;
mod interface;
pub mod names;
mod parse;
mod select;
pub mod serialize;
mod style;
mod tree;
mod write;

pub use dom::{HtmlDom, HtmlNode};
pub use write::HtmlWriter;

use tarnish::dom::{DomParser, DomSerializer, ParseOptions};
use tarnish::{Fragment, Node, Result, Slice};

/// Parse HTML into a document: `parser.parse` of a `<template>`'s content, the HTML parsed
/// into it.
pub fn parse_html(
    parser: &DomParser<HtmlNode>,
    html: &str,
    options: ParseOptions<'_, HtmlNode>,
) -> Result<Node<'static>> {
    let dom = HtmlDom::new();
    parser.parse(&dom, &dom.parse_fragment(html), options)
}

/// Parse HTML into a slice: `parser.parseSlice` of a `<template>`'s content, the HTML parsed
/// into it.
pub fn parse_html_slice(
    parser: &DomParser<HtmlNode>,
    html: &str,
    options: ParseOptions<'_, HtmlNode>,
) -> Result<Slice<'static>> {
    let dom = HtmlDom::new();
    parser.parse_slice(&dom, &dom.parse_fragment(html), options)
}

/// Write a fragment as HTML: the `innerHTML` of an element `serializeFragment` fills, written as
/// the serializer goes.
pub fn to_html(
    serializer: &DomSerializer<HtmlNode>,
    fragment: &Fragment<'static>,
) -> Result<String> {
    let mut writer = HtmlWriter::new();
    serializer.write_fragment(fragment, &mut writer)?;
    Ok(writer.finish())
}

#[cfg(test)]
mod tests {
    use tarnish::dom::{DomParser, DomSerializer};

    use super::HtmlNode;

    #[test]
    fn parsers_and_serializers_can_be_shared_between_threads() {
        fn shared<T: Send + Sync>() {}
        shared::<DomParser<HtmlNode>>();
        shared::<DomSerializer<HtmlNode>>();
    }
}
