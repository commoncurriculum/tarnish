//! `@tiptap/extension-subscript`.

use crate::DomSpec;
use crate::{GetAttrsResult, MarkExtension, ParseHtml};

pub fn subscript() -> MarkExtension {
    MarkExtension::create("subscript")
        .parse_html(vec![
            ParseHtml::tag("sub").into(),
            ParseHtml::style("vertical-align")
                .get_attrs(|value| {
                    Ok(if value == "sub" {
                        GetAttrsResult::Defaults
                    } else {
                        GetAttrsResult::Reject
                    })
                })
                .into(),
        ])
        .render_html(|_, html| DomSpec::wrapping("sub", html))
}
