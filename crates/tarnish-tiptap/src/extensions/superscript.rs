//! `@tiptap/extension-superscript`.

use crate::DomSpec;
use crate::merge_attributes::merge_configured;
use crate::{GetAttrsResult, MarkExtension, ParseHtml};
use tarnish_js::json::Map;

#[derive(Default)]
pub struct SuperscriptOptions {
    pub html_attributes: Map,
}

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
        .render_html(move |_, html| {
            let html = merge_configured(&options.html_attributes, html, [])?;
            Ok(DomSpec::wrapping("sup", html))
        })
}
