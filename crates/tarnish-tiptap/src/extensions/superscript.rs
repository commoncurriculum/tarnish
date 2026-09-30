//! `@tiptap/extension-superscript`.

use crate::DomSpec;
use crate::GetAttrs;
use crate::{MarkExtension, ParseHtml};

pub fn superscript() -> MarkExtension {
    MarkExtension::create("superscript")
        .parse_html(vec![
            ParseHtml::tag("sup").into(),
            ParseHtml::style("vertical-align")
                .get_attrs(|value| match value == "super" {
                    true => GetAttrs::Null,
                    false => GetAttrs::False,
                })
                .into(),
        ])
        .render_html(|_, html| DomSpec::wrapping("sup", html))
}
