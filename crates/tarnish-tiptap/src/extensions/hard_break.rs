//! `@tiptap/extension-hard-break`.

use crate::markdown::{Parsed, RenderContext, RenderHelpers};
use crate::merge_attributes::render_element;
use crate::{HtmlAttributesOptions, NodeExtension, ParseHtml};
use tarnish_js::Error;
use tarnish_js::json::{Value, json};

/// HardBreak's options but `keepMarks`, which only its command reads.
pub type HardBreakOptions = HtmlAttributesOptions;

pub fn hard_break(options: HardBreakOptions) -> NodeExtension {
    NodeExtension::create("hardBreak")
        .markdown_token_name("br")
        .inline(true)
        .group("inline")
        .selectable(false)
        .linebreak_replacement()
        .parse_html([ParseHtml::tag("br")])
        .render_html(render_element("br", options.html_attributes, []))
        .render_text(|_| Ok("\n".to_owned()))
        .render_markdown(render_markdown)
        .parse_markdown(|_, _| Ok(Parsed::Node(json!({ "type": "hardBreak" }))))
}

fn render_markdown(_: &Value, _: &dyn RenderHelpers, _: &RenderContext) -> Result<String, Error> {
    Ok("  \n".to_string())
}
