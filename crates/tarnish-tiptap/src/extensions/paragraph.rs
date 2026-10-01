//! `@tiptap/extension-paragraph`.

use crate::DomSpec;
use crate::markdown::{ParseHelpers, Parsed, RenderContext, RenderHelpers};
use crate::{NodeExtension, ParseHtml};
use tarnish::json::Value;
use tarnish_js::Error;
use tarnish_js::{self as js, utf16};
use tarnish_markdown::marked::Token;

pub const EMPTY_PARAGRAPH_MARKDOWN: &str = "&nbsp;";

pub fn paragraph() -> NodeExtension {
    NodeExtension::create("paragraph")
        .priority(1000)
        .group("block")
        .content("inline*")
        .parse_html([ParseHtml::tag("p")])
        .render_html(|_, html| Ok(DomSpec::wrapping("p", html)))
        .parse_markdown(parse_markdown)
        .render_markdown(render_markdown)
}

const NBSP_CHAR: &str = "\u{A0}";

fn parse_markdown(token: &Token, helpers: &ParseHelpers) -> Result<Parsed, Error> {
    let tokens = token.tokens.as_deref().unwrap_or_default();
    if let [only] = tokens
        && only.kind == "image"
    {
        return Ok(Parsed::Nodes(helpers.parse_children(tokens)?));
    }
    let content = helpers.parse_inline(tokens)?;
    let is_marker = |text: &[u16]| utf16::is(text, EMPTY_PARAGRAPH_MARKDOWN) || text == [0xA0];
    let has_explicit_empty_paragraph_marker = matches!(
        tokens,
        [only] if only.kind == "text" && (is_marker(&only.raw) || is_marker(only.text()))
    );
    if has_explicit_empty_paragraph_marker && let [Value::Object(text)] = content.as_slice() {
        let is_text = text.get("type").and_then(Value::as_str) == Some("text");
        let marker = text.get("text").and_then(Value::as_str);
        if is_text && matches!(marker, Some(EMPTY_PARAGRAPH_MARKDOWN | NBSP_CHAR)) {
            return Ok(Parsed::Node(helpers.create_node(
                "paragraph",
                None,
                Some(Vec::new()),
            )));
        }
    }
    Ok(Parsed::Node(helpers.create_node(
        "paragraph",
        None,
        Some(content),
    )))
}

fn render_markdown(
    node: &Value,
    helpers: &dyn RenderHelpers,
    context: &RenderContext,
) -> Result<String, Error> {
    let content = js::array(node.get("content"));
    if content.is_empty() {
        let previous_node_is_empty_paragraph = context.previous_node.is_some_and(|previous| {
            previous.get("type").and_then(Value::as_str) == Some("paragraph")
                && js::array(previous.get("content")).is_empty()
        });
        return Ok(if previous_node_is_empty_paragraph {
            EMPTY_PARAGRAPH_MARKDOWN.to_string()
        } else {
            String::new()
        });
    }
    helpers.render_array(content, "")
}
