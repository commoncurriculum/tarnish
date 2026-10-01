//! `@tiptap/extension-highlight`.

use std::sync::LazyLock;

use crate::DomSpec;
use crate::markdown::{RenderContext, RenderHelpers};
use crate::{ExtensionAttribute, MarkExtension, ParseHtml, get_style_property};
use tarnish_js::Error;
use tarnish_js::json::Value;
use tarnish_js::regexp::RegExp;
use tarnish_js::units::Units;
use tarnish_js::utf16;
use tarnish_markdown::marked::{Lexer, Token};

pub struct HighlightOptions {
    pub multicolor: bool,
}

pub fn highlight(options: HighlightOptions) -> MarkExtension {
    let attributes = if options.multicolor {
        vec![color()]
    } else {
        Vec::new()
    };
    MarkExtension::create("highlight")
        .add_attributes(attributes)
        .parse_html([ParseHtml::tag("mark")])
        .render_html(|_, html| Ok(DomSpec::wrapping("mark", html)))
        .render_markdown(render_markdown)
        .parse_markdown(|token, helpers| {
            let content = helpers.parse_inline(token.tokens.as_deref().unwrap_or_default())?;
            Ok(helpers.apply_mark("highlight", content, None))
        })
        .markdown_tokenizer("==", tokenize)
}

fn render_markdown(
    node: &Value,
    helpers: &dyn RenderHelpers,
    _: &RenderContext,
) -> Result<String, Error> {
    Ok(["==", &helpers.render_children(node, "")?, "=="].concat())
}

static RULE: LazyLock<RegExp> = LazyLock::new(|| RegExp::new("^(==)([^=]+)(==)", ""));

fn tokenize(src: &Units, _: &[Token], lexer: &mut Lexer) -> Result<Option<Token>, Error> {
    let Some(found) = RULE.exec(src) else {
        return Ok(None);
    };
    let inner_content = src.slice_of(utf16::trim(found.get(2).unwrap_or_default()));
    let children = lexer.inline_tokens(&inner_content)?;
    Ok(Some(Token {
        text: Some(inner_content),
        tokens: Some(children.into()),
        ..Token::new("highlight", src.slice_of(found.all()))
    }))
}

fn color() -> ExtensionAttribute {
    ExtensionAttribute::new("color", Value::Null)
        .parse_html(|element| {
            let value = element
                .attribute("data-color")
                .filter(|color| !color.is_empty())
                .or_else(|| {
                    get_style_property(element, "background-color")
                        .filter(|color| !color.is_empty())
                })
                .unwrap_or_else(|| element.style_value("background-color"));
            Some(Value::String(value))
        })
        .render_html(|attributes, rendered| match attributes.get("color") {
            Some(color) if color.truthy() => {
                let style = format!(
                    "background-color: {}; color: inherit",
                    color.to_js_string()?
                );
                rendered.merge("data-color", color)?;
                rendered.merge("style", style)
            }
            _ => Ok(()),
        })
}
