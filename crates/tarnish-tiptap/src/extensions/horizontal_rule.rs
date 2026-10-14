//! `@tiptap/extension-horizontal-rule`.

use crate::markdown::Parsed;
use crate::merge_attributes::merge_configured;
use crate::{DomSpec, NodeExtension, ParseHtml};
use tarnish_js::json::Map;

#[derive(Default)]
pub struct HorizontalRuleOptions {
    pub html_attributes: Map,
}

pub fn horizontal_rule(options: HorizontalRuleOptions) -> NodeExtension {
    NodeExtension::create("horizontalRule")
        .group("block")
        .parse_html([ParseHtml::tag("hr")])
        .render_html(move |_, html| {
            let html = merge_configured(&options.html_attributes, html, [])?;
            Ok(DomSpec::element("hr", html, Vec::new()))
        })
        .markdown_token_name("hr")
        .parse_markdown(|_, helpers| {
            Ok(Parsed::Node(helpers.create_node(
                "horizontalRule",
                None,
                None,
            )))
        })
        .render_markdown(|_, _, _| Ok("---".to_string()))
}
