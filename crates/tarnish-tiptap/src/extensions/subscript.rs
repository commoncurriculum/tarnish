//! `@tiptap/extension-subscript`.

use crate::merge_attributes::render_wrapping;
use crate::{GetAttrsResult, HtmlAttributesOptions, MarkExtension, ParseHtml};

pub type SubscriptOptions = HtmlAttributesOptions;

pub fn subscript(options: SubscriptOptions) -> MarkExtension {
    MarkExtension::create("subscript")
        .parse_html([
            ParseHtml::Tag(ParseHtml::tag("sub")),
            ParseHtml::Style(ParseHtml::style("vertical-align").get_attrs(|value| {
                Ok(if value == "sub" {
                    GetAttrsResult::Defaults
                } else {
                    GetAttrsResult::Reject
                })
            })),
        ])
        .render_html(render_wrapping("sub", options.html_attributes, []))
}
