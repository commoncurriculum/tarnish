//! `@tiptap/extension-text`.

use crate::NodeExtension;
use crate::markdown::{ParseHelpers, Parsed, RenderContext, RenderHelpers};
use tarnish::json::{Value, json};
use tarnish_js as js;
use tarnish_js::Error;
use tarnish_js::utf16;
use tarnish_markdown::marked::Token;

pub fn text() -> NodeExtension {
    NodeExtension::create("text")
        .group("inline")
        .parse_markdown(parse_markdown)
        .render_markdown(render_markdown)
}

fn parse_markdown(token: &Token, _: &dyn ParseHelpers) -> Result<Parsed, Error> {
    Ok(Parsed::Node(
        json!({ "type": "text", "text": utf16::to_string(token.text()) }),
    ))
}

fn render_markdown(
    node: &Value,
    _: &dyn RenderHelpers,
    _: &RenderContext,
) -> Result<String, Error> {
    Ok(match node.get("text") {
        Some(text) if js::truthy(Some(text)) => js::to_string(text)?,
        _ => String::new(),
    })
}
