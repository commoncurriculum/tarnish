//! `@tiptap/extension-underline`.

use std::sync::LazyLock;

use crate::DomSpec;
use crate::GetAttrs;
use crate::{MarkExtension, ParseHtml};
use tarnish::Map as Attrs;
use tarnish_js::Error;
use tarnish_js::regexp::RegExp;
use tarnish_js::units::Units;
use tarnish_js::utf16;
use tarnish_markdown::marked::{Lexer, Token};

pub fn underline() -> MarkExtension {
    MarkExtension::create("underline")
        .parse_html(vec![
            ParseHtml::tag("u").into(),
            ParseHtml::style("text-decoration")
                .not_consuming()
                .get_attrs(|value| match value.contains("underline") {
                    true => GetAttrs::Attrs(Attrs::new()),
                    false => GetAttrs::False,
                })
                .into(),
        ])
        .render_html(|_, html| DomSpec::wrapping("u", html))
        .markdown_tokenizer("++", tokenize)
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
