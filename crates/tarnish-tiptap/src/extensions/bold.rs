//! `@tiptap/extension-bold`.

use crate::DomSpec;
use crate::GetAttrs;
use crate::markdown::{RenderContext, RenderHelpers};
use crate::{MarkExtension, ParseHtml};
use tarnish::Map as Attrs;
use tarnish::json::Value;
use tarnish_js::Error;

pub fn bold() -> MarkExtension {
    MarkExtension::create("bold")
        .parse_html(vec![
            ParseHtml::tag("strong").into(),
            ParseHtml::tag("b")
                .get_attrs(|b| (b.style_value("font-weight") != "normal").then(Attrs::new))
                .into(),
            ParseHtml::style("font-weight=400").clears_mark().into(),
            ParseHtml::style("font-weight")
                .get_attrs(|value| match is_bold_weight(value) {
                    true => GetAttrs::Null,
                    false => GetAttrs::False,
                })
                .into(),
        ])
        .render_html(|_, html| DomSpec::wrapping("strong", html))
        .markdown_token_name("strong")
        .html_reopen("<strong>", "</strong>")
        .parse_markdown(|token, helpers| {
            let content = helpers.parse_inline(token.tokens.as_deref().unwrap_or_default())?;
            Ok(helpers.apply_mark("bold", content, None))
        })
        .render_markdown(render_markdown)
}

fn render_markdown(
    node: &Value,
    helpers: &dyn RenderHelpers,
    _: &RenderContext,
) -> Result<String, Error> {
    Ok(["**", &helpers.render_children(node, "")?, "**"].concat())
}

/// `/^(bold(er)?|[5-9]\d{2,})$/`.
fn is_bold_weight(value: &str) -> bool {
    matches!(value, "bold" | "bolder")
        || (value.len() >= 3
            && value.starts_with(['5', '6', '7', '8', '9'])
            && value.bytes().all(|byte| byte.is_ascii_digit()))
}
