//! `@tiptap/extension-superscript`.

use crate::DomSpec;
use crate::{GetAttrsResult, MarkExtension, ParseHtml};

pub fn superscript() -> MarkExtension {
    MarkExtension::create("superscript")
        .parse_html(vec![
            ParseHtml::tag("sup").into(),
            ParseHtml::style("vertical-align")
                .get_attrs(|value| {
                    Ok(if value == "super" {
                        GetAttrsResult::Defaults
                    } else {
                        GetAttrsResult::Reject
                    })
                })
                .into(),
        ])
        .render_html(|_, html| DomSpec::wrapping("sup", html))
}
