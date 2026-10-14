//! `@tiptap/extension-hard-break`.

use crate::DomSpec;
use crate::markdown::{Parsed, RenderContext, RenderHelpers};
use crate::merge_attributes::merge_configured;
use crate::{NodeExtension, ParseHtml};
use tarnish_js::Error;
use tarnish_js::json::{Map, Value, json};

/// HardBreak's options but `keepMarks`, which only its command reads.
#[derive(Default)]
pub struct HardBreakOptions {
    pub html_attributes: Map,
}

pub fn hard_break(options: HardBreakOptions) -> NodeExtension {
    NodeExtension::create("hardBreak")
        .markdown_token_name("br")
        .inline(true)
        .group("inline")
        .selectable(false)
        .linebreak_replacement()
        .parse_html([ParseHtml::tag("br")])
        .render_html(move |_, html| {
            let html = merge_configured(&options.html_attributes, html, [])?;
            Ok(DomSpec::element("br", html, Vec::new()))
        })
        .render_text(|_| Ok("\n".to_owned()))
        .render_markdown(render_markdown)
        .parse_markdown(|_, _| Ok(Parsed::Node(json!({ "type": "hardBreak" }))))
}

fn render_markdown(_: &Value, _: &dyn RenderHelpers, _: &RenderContext) -> Result<String, Error> {
    Ok("  \n".to_string())
}
