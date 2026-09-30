//! HTML to documents and back, as a browser's DOM does it.

use tarnish::dom::ParseOptions;
use tarnish::{Node, Result};
use tarnish_html::HtmlDom;

use crate::TiptapSchema;

/// `DOMParser.fromSchema(schema).parse(document.body)`, of a document whose body is the HTML.
pub fn parse(schema: &TiptapSchema, html: &str) -> Result<Node<'static>> {
    let dom = HtmlDom::parse_document(&format!("<!DOCTYPE html><html><body>{html}</body></html>"));
    let body = dom.body().expect("a parsed document has a body");
    schema.parser.parse(&dom, &body, ParseOptions::default())
}

/// The `innerHTML` of an element `DOMSerializer.fromSchema(schema).serializeFragment` fills with
/// the document's content.
pub fn serialize(schema: &TiptapSchema, document: &Node<'static>) -> Result<String> {
    tarnish_html::to_html(&schema.serializer, document.content())
}
