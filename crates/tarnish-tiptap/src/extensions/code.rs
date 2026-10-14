//! `@tiptap/extension-code`: the inline code mark, which no other mark can be beside.

use crate::markdown::{RenderContext, RenderHelpers};
use crate::merge_attributes::merge_configured;
use crate::{DomSpec, MarkExtension, ParseHtml};
use tarnish_js::json::{Map, Value, json};
use tarnish_js::{self as js, Error, utf16};

#[derive(Default)]
pub struct CodeOptions {
    pub html_attributes: Map,
}

pub fn code(options: CodeOptions) -> MarkExtension {
    MarkExtension::create("code")
        .excludes("_")
        .code()
        .parse_html([ParseHtml::tag("code")])
        .render_html(move |_, html| {
            Ok(DomSpec::wrapping(
                "code",
                merge_configured(&options.html_attributes, html, [])?,
            ))
        })
        .markdown_token_name("codespan")
        .parse_markdown(|token, helpers| {
            let text = json!({ "type": "text", "text": utf16::to_string(token.text()) });
            Ok(helpers.apply_mark("code", vec![text], None))
        })
        .render_markdown(render_markdown)
}

/// Its text between single backticks, whatever backticks the text holds.
fn render_markdown(
    node: &Value,
    helpers: &dyn RenderHelpers,
    _: &RenderContext,
) -> Result<String, Error> {
    match node.get("content") {
        Some(content) if js::truthy(Some(content)) => {
            Ok(format!("`{}`", helpers.render_children(content, "")?))
        }
        _ => Ok(String::new()),
    }
}
