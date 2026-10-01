//! `@tiptap/extension-strike`.

use crate::DomSpec;
use crate::markdown::{RenderContext, RenderHelpers};
use crate::{GetAttrsResult, MarkExtension, ParseHtml};
use tarnish::Map as Attrs;
use tarnish::json::Value;
use tarnish_js::Error;

pub fn strike() -> MarkExtension {
    MarkExtension::create("strike")
        .parse_html([
            ParseHtml::Tag(ParseHtml::tag("s")),
            ParseHtml::Tag(ParseHtml::tag("del")),
            ParseHtml::Tag(ParseHtml::tag("strike")),
            ParseHtml::Style(
                ParseHtml::style("text-decoration")
                    .not_consuming()
                    .get_attrs(|value| {
                        Ok(if value.contains("line-through") {
                            GetAttrsResult::Attrs(Attrs::new())
                        } else {
                            GetAttrsResult::Reject
                        })
                    }),
            ),
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
