//! `@tiptap/extension-image`.

use crate::markdown::{ParseHelpers, Parsed, RenderContext, RenderHelpers};
use crate::merge_attributes::render_element;
use crate::{ExtensionAttribute, NodeExtension, ParseHtml};
use tarnish_js::json::{Map, Value, json};
use tarnish_js::{self as js, Error, utf16, value};
use tarnish_markdown::marked::Token;

/// Image's options but `resize`, which only its node view reads.
#[derive(Default)]
pub struct ImageOptions {
    pub inline: bool,
    /// Whether an `img` whose `src` is a `data:` URL is an image.
    pub allow_base64: bool,
    pub html_attributes: Map,
}

pub fn image(options: ImageOptions) -> NodeExtension {
    let attributes = ["src", "alt", "title", "width", "height"]
        .map(|name| ExtensionAttribute::new(name, Value::Null));
    NodeExtension::create("image")
        .inline_or_block(options.inline)
        .draggable()
        .add_attributes(attributes.into())
        .parse_html([ParseHtml::tag(if options.allow_base64 {
            "img[src]"
        } else {
            r#"img[src]:not([src^="data:"])"#
        })])
        .render_html(render_element("img", options.html_attributes, []))
        .parse_markdown(parse_markdown)
        .render_markdown(render_markdown)
}

fn parse_markdown(token: &Token, helpers: &ParseHelpers) -> Result<Parsed, Error> {
    let string = |units: &[u16]| json!(utf16::to_string(units));
    // `{ src: token.href, title: token.title, alt: token.text }`, where marked's title is null
    // when the link has none.
    let mut attrs = Map::with_capacity(3);
    if let Some(href) = token.href() {
        attrs.push("src".into(), string(href));
    }
    attrs.push("title".into(), Value::from(token.title().map(string)));
    if let Some(text) = &token.text {
        attrs.push("alt".into(), string(text));
    }
    Ok(Parsed::Node(helpers.create_node(
        "image",
        Some(attrs),
        None,
    )))
}

fn render_markdown(
    node: &Value,
    _: &dyn RenderHelpers,
    _: &RenderContext,
) -> Result<String, Error> {
    // `node.attrs?.[key] ?? ''`
    let attr = |key| js::coalesce(value::optional(node.get("attrs"), key), &js::EMPTY_STRING);
    let (src, alt, title) = (attr("src"), attr("alt"), attr("title"));
    let image = format!("![{}]({}", js::to_string(alt)?, js::to_string(src)?);
    Ok(if js::truthy(Some(title)) {
        format!("{image} \"{}\")", js::to_string(title)?)
    } else {
        image + ")"
    })
}
