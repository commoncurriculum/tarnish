//! `@tiptap/extension-superscript`.

use crate::merge_attributes::render_wrapping;
use crate::{GetAttrsResult, HtmlAttributesOptions, MarkExtension, ParseHtml};

pub type SuperscriptOptions = HtmlAttributesOptions;

pub fn superscript(options: SuperscriptOptions) -> MarkExtension {
    MarkExtension::create("superscript")
        .parse_html([
            ParseHtml::Tag(ParseHtml::tag("sup")),
            ParseHtml::Style(ParseHtml::style("vertical-align").get_attrs(|value| {
                Ok(if value == "super" {
                    GetAttrsResult::Defaults
                } else {
                    GetAttrsResult::Reject
                })
            })),
        ])
        .render_html(render_wrapping("sup", options.html_attributes, []))
}
