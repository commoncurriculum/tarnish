//! `@tiptap/extension-strike`.

use crate::DomSpec;
use crate::GetAttrs;
use crate::markdown::{RenderContext, RenderHelpers};
use crate::{MarkExtension, ParseHtml};
use tarnish::Map as Attrs;
use tarnish::json::Value;
use tarnish_js::Error;

pub fn strike() -> MarkExtension {
    MarkExtension::create("strike")
        .parse_html(vec![
            ParseHtml::tag("s").into(),
            ParseHtml::tag("del").into(),
            ParseHtml::tag("strike").into(),
            ParseHtml::style("text-decoration")
                .not_consuming()
                .get_attrs(|value| match value.contains("line-through") {
                    true => GetAttrs::Attrs(Attrs::new()),
                    false => GetAttrs::False,
                })
                .into(),
        ])
        .render_html(|_, html| DomSpec::wrapping("s", html))
        .markdown_token_name("del")
        .parse_markdown(|token, helpers| {
            let content = helpers.parse_inline(token.tokens.as_deref().unwrap_or_default())?;
            Ok(helpers.apply_mark("strike", content, None))
        })
        .render_markdown(render_markdown)
}

fn render_markdown(
    node: &Value,
    helpers: &dyn RenderHelpers,
    _: &RenderContext,
) -> Result<String, Error> {
    Ok(["~~", &helpers.render_children(node, "")?, "~~"].concat())
}
