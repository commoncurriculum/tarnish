//! `@tiptap/extension-document`.

use crate::NodeExtension;
use crate::markdown::{RenderContext, RenderHelpers};
use tarnish_js as js;
use tarnish_js::Error;
use tarnish_js::json::Value;

pub fn document() -> NodeExtension {
    NodeExtension::create("doc")
        .content("block+")
        .render_markdown(render_markdown)
}

fn render_markdown(
    node: &Value,
    helpers: &dyn RenderHelpers,
    _: &RenderContext,
) -> Result<String, Error> {
    match node.get("content") {
        Some(content) if js::truthy(Some(content)) => helpers.render_children(content, "\n\n"),
        _ => Ok(String::new()),
    }
}
