//! `@tiptap/extension-italic`.

use crate::DomSpec;
use crate::markdown::{RenderContext, RenderHelpers};
use crate::{MarkExtension, ParseHtml};
use tarnish::Map as Attrs;
use tarnish::json::Value;
use tarnish_js::Error;

pub fn italic() -> MarkExtension {
    MarkExtension::create("italic")
        .parse_html(vec![
            ParseHtml::tag("em").into(),
            ParseHtml::tag("i")
                .get_attrs(|i| Ok((i.style_value("font-style") != "normal").then(Attrs::new)))
                .into(),
            ParseHtml::style("font-style=normal").clears_mark().into(),
            ParseHtml::style("font-style=italic").into(),
        ])
        .render_html(|_, html| DomSpec::wrapping("em", html))
        .markdown_token_name("em")
        .html_reopen("<em>", "</em>")
        .parse_markdown(|token, helpers| {
            let content = helpers.parse_inline(token.tokens.as_deref().unwrap_or_default())?;
            Ok(helpers.apply_mark("italic", content, None))
        })
        .render_markdown(render_markdown)
}

fn render_markdown(
    node: &Value,
    helpers: &dyn RenderHelpers,
    _: &RenderContext,
) -> Result<String, Error> {
    Ok(["*", &helpers.render_children(node, "")?, "*"].concat())
}
