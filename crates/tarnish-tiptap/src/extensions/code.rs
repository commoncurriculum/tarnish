//! `@tiptap/extension-code`: the inline code mark, which no other mark can be beside.

use crate::markdown::{RenderContext, RenderHelpers};
use crate::merge_attributes::render_wrapping;
use crate::{HtmlAttributesOptions, MarkExtension, ParseHtml};
use tarnish_js::json::{Value, json};
use tarnish_js::{self as js, Error, utf16};

pub type CodeOptions = HtmlAttributesOptions;

pub fn code(options: CodeOptions) -> MarkExtension {
    MarkExtension::create("code")
        .excludes("_")
        .code()
        .parse_html([ParseHtml::tag("code")])
        .render_html(render_wrapping("code", options.html_attributes, []))
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
