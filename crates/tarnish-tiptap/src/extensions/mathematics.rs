//! `@tiptap/extension-mathematics`: `InlineMath` and `BlockMath`, which hold their LaTeX in an
//! attribute, and `Mathematics`, which adds both. KaTeX renders the LaTeX in their node views
//! only, so the HTML holds the LaTeX alone.

use std::sync::LazyLock;

use crate::markdown::{Parsed, RenderContext, RenderHelpers};
use crate::{
    DomSpec, Extension, ExtensionAttribute, NodeExtension, ParseHtml, PlainExtension,
    merge_attributes,
};
use tarnish_js::json::{Value, json};
use tarnish_js::regexp::RegExp;
use tarnish_js::units::Units;
use tarnish_js::{self as js, Error, utf16, value};
use tarnish_markdown::marked::{Lexer, Token};

/// `Mathematics`, with the extensions it adds after it.
pub fn mathematics() -> Vec<Extension> {
    vec![
        PlainExtension::create("Mathematics").into(),
        block_math().into(),
        inline_math().into(),
    ]
}

fn latex() -> ExtensionAttribute {
    ExtensionAttribute::new("latex", json!("")).html_attribute("data-latex")
}

pub fn inline_math() -> NodeExtension {
    NodeExtension::create("inlineMath")
        .group("inline")
        .inline(true)
        .atom()
        .add_attributes(vec![latex()])
        .parse_html([ParseHtml::tag(r#"span[data-type="inline-math"]"#)])
        .render_html(|_, html| {
            let html = merge_attributes(html, [("data-type", "inline-math".into())])?;
            Ok(DomSpec::element("span", html, Vec::new()))
        })
        .parse_markdown(|token, _| Ok(Parsed::Node(math("inlineMath", token))))
        .render_markdown(|node, _: &dyn RenderHelpers, _: &RenderContext| {
            Ok(format!("${}$", latex_of(node)?))
        })
        .markdown_tokenizer("$", tokenize_inline)
}

pub fn block_math() -> NodeExtension {
    NodeExtension::create("blockMath")
        .group("block")
        .atom()
        .add_attributes(vec![latex()])
        .parse_html([ParseHtml::tag(r#"div[data-type="block-math"]"#)])
        .render_html(|_, html| {
            let html = merge_attributes(html, [("data-type", "block-math".into())])?;
            Ok(DomSpec::element("div", html, Vec::new()))
        })
        .parse_markdown(|token, _| Ok(Parsed::Node(math("blockMath", token))))
        .render_markdown(|node, _: &dyn RenderHelpers, _: &RenderContext| {
            Ok(["$$", &latex_of(node)?, "$$"].join("\n"))
        })
        .markdown_block_tokenizer(|src| utf16::index_of_str(src, "$$", 0), tokenize_block)
}

/// `{ type, attrs: { latex: token.latex } }`.
fn math(kind: &str, token: &Token) -> Value {
    let mut attrs = tarnish_js::json::Map::new();
    if let Some(latex) = token.extra("latex") {
        attrs.push("latex".into(), latex.clone());
    }
    json!({ "type": kind, "attrs": attrs })
}

/// `node.attrs?.latex || ''`.
fn latex_of(node: &Value) -> Result<String, Error> {
    js::string_or_empty(value::optional(node.get("attrs"), "latex"))
}

static INLINE: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r"^\$([^$]+)\$(?!\$)", ""));
static BLOCK: LazyLock<RegExp> = LazyLock::new(|| RegExp::new(r"^\$\$([^$]+)\$\$", ""));

fn tokenize_inline(src: &Units, _: &[Token], _: &mut Lexer) -> Result<Option<Token>, Error> {
    Ok(tokenize(&INLINE, "inlineMath", src))
}

fn tokenize_block(src: &Units, _: &[Token], _: &mut Lexer) -> Result<Option<Token>, Error> {
    Ok(tokenize(&BLOCK, "blockMath", src))
}

/// `{ type, raw, latex: latex.trim() }` of the LaTeX between the dollar signs.
fn tokenize(rule: &RegExp, kind: &'static str, src: &Units) -> Option<Token> {
    let found = rule.exec(src)?;
    let latex = utf16::to_string(utf16::trim(found.get(1).unwrap_or_default()));
    Some(Token {
        extra: Some(Box::new(tarnish_js::json::Map::from_iter([(
            "latex".into(),
            json!(latex),
        )]))),
        ..Token::new(kind, src.slice_of(found.all()))
    })
}
