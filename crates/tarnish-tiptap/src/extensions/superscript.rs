//! `@tiptap/extension-superscript`.

use crate::DomSpec;
use crate::{GetAttrsResult, MarkExtension, ParseHtml};

pub fn superscript() -> MarkExtension {
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
        .render_html(|_, html| Ok(DomSpec::wrapping("sup", html)))
}
