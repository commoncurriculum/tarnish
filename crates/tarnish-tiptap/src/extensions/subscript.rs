//! `@tiptap/extension-subscript`.

use crate::DomSpec;
use crate::merge_attributes::merge_configured;
use crate::{GetAttrsResult, MarkExtension, ParseHtml};
use tarnish_js::json::Map;

#[derive(Default)]
pub struct SubscriptOptions {
    pub html_attributes: Map,
}

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
        .render_html(move |_, html| {
            let html = merge_configured(&options.html_attributes, html, [])?;
            Ok(DomSpec::wrapping("sub", html))
        })
}
