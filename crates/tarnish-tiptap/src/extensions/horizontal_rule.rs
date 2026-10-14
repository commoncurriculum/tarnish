//! `@tiptap/extension-horizontal-rule`.

use crate::markdown::Parsed;
use crate::merge_attributes::render_element;
use crate::{HtmlAttributesOptions, NodeExtension, ParseHtml};

pub type HorizontalRuleOptions = HtmlAttributesOptions;

pub fn horizontal_rule(options: HorizontalRuleOptions) -> NodeExtension {
    NodeExtension::create("horizontalRule")
        .group("block")
        .parse_html([ParseHtml::tag("hr")])
        .render_html(render_element("hr", options.html_attributes, []))
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
