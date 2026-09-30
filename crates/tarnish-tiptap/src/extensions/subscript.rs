//! `@tiptap/extension-subscript`.

use crate::DomSpec;
use crate::GetAttrs;
use crate::{MarkExtension, ParseHtml};

pub fn subscript() -> MarkExtension {
    MarkExtension::create("subscript")
        .parse_html(vec![
            ParseHtml::tag("sub").into(),
            ParseHtml::style("vertical-align")
                .get_attrs(|value| match value == "sub" {
                    true => GetAttrs::Null,
                    false => GetAttrs::False,
                })
                .into(),
        ])
        .render_html(|_, html| DomSpec::wrapping("sub", html))
}
