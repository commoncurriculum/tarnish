//! `@tiptap/extension-underline`.

use std::sync::LazyLock;

use crate::DomSpec;
use crate::markdown::{RenderContext, RenderHelpers};
use crate::{GetAttrsResult, MarkExtension, ParseHtml};
use tarnish::Map as Attrs;
use tarnish::json::Value;
use tarnish_js::Error;
use tarnish_js::regexp::RegExp;
use tarnish_js::units::Units;
use tarnish_js::utf16;
use tarnish_markdown::marked::{Lexer, Token};

pub fn underline() -> MarkExtension {
    MarkExtension::create("underline")
        .parse_html([
            ParseHtml::Tag(ParseHtml::tag("u")),
            ParseHtml::Style(
                ParseHtml::style("text-decoration")
                    .not_consuming()
                    .get_attrs(|value| {
                        Ok(if value.contains("underline") {
                            GetAttrsResult::Attrs(Attrs::new())
                        } else {
                            GetAttrsResult::Reject
                        })
                    }),
            ),
        ])
        .render_html(|_, html| DomSpec::wrapping("u", html))
        .parse_markdown(|token, helpers| {
            let content = helpers.parse_inline(token.tokens.as_deref().unwrap_or_default())?;
            Ok(helpers.apply_mark("underline", content, None))
        })
        .render_markdown(render_markdown)
        .markdown_tokenizer("++", tokenize)
}

fn render_markdown(
    node: &Value,
    helpers: &dyn RenderHelpers,
    _: &RenderContext,
) -> Result<String, Error> {
    Ok(["++", &helpers.render_children(node, "")?, "++"].concat())
}

static RULE: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r"^(\+\+)([\s\S]+?)(\+\+)", ""));

fn tokenize(src: &Units, _: &[Token], lexer: &mut Lexer) -> Result<Option<Token>, Error> {
    let Some(found) = RULE.exec(src) else {
        return Ok(None);
    };
    let inner_content = src.slice_of(utf16::trim(found.get(2).unwrap_or_default()));
    let tokens = lexer.inline_tokens(&inner_content)?;
    Ok(Some(Token {
        text: Some(inner_content),
        tokens: Some(tokens.into()),
        ..Token::new("underline", src.slice_of(found.all()))
    }))
}
