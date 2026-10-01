//! `@tiptap/extension-hard-break`.

use crate::DomSpec;
use crate::markdown::{Parsed, RenderContext, RenderHelpers};
use crate::{NodeExtension, ParseHtml};
use tarnish::json::{Value, json};
use tarnish_js::Error;

pub fn hard_break() -> NodeExtension {
    NodeExtension::create("hardBreak")
        .markdown_token_name("br")
        .inline(true)
        .group("inline")
        .linebreak_replacement()
        .parse_html([ParseHtml::tag("br")])
        .render_html(|_, html| Ok(DomSpec::element("br", html, Vec::new())))
        .render_markdown(render_markdown)
        .parse_markdown(|_, _| Ok(Parsed::Node(json!({ "type": "hardBreak" }))))
}

fn render_markdown(_: &Value, _: &dyn RenderHelpers, _: &RenderContext) -> Result<String, Error> {
    Ok("  \n".to_string())
}
