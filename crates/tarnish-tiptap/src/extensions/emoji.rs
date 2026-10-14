//! `@tiptap/extension-emoji`'s node, which names its emoji by shortcode and renders the emoji
//! of the list the application gives: Tiptap's own list is the data `emojis` it exports. A
//! server can't draw an emoji to see whether it shows, so every emoji with an image renders
//! as its image, as `isEmojiSupported` has it outside a browser.

use crate::markdown::{RenderContext, RenderHelpers};
use crate::{
    AttrValue, DomSpec, ExtensionAttribute, NodeExtension, ParseHtml, SpecAttrs, element_attrs,
    merge_objects, node_attr, rendered_object,
};
use tarnish::chunk::ValueRef;
use tarnish_js::json::{Map, Value, json};
use tarnish_js::{self as js, Error, value};

/// `EmojiItem`, as far as the node renders it.
#[derive(Clone, Default)]
pub struct EmojiItem {
    pub name: String,
    pub emoji: Option<String>,
    pub shortcodes: Vec<String>,
    pub fallback_image: Option<String>,
}

#[derive(Default)]
pub struct EmojiOptions {
    pub html_attributes: Map,
    pub emojis: Vec<EmojiItem>,
    pub force_fallback_images: bool,
}

pub fn emoji(options: EmojiOptions) -> NodeExtension {
    let emojis = options.emojis.clone();
    NodeExtension::create("emoji")
        .inline(true)
        .group("inline")
        .selectable(false)
        .add_attributes(vec![
            ExtensionAttribute::new("name", Value::Null)
                .parse_html(|element| element.attribute("data-name").map(Value::String))
                .render_html(|attributes, rendered| {
                    rendered.merge("data-name", attributes.get("name").expect("its attribute"))
                }),
        ])
        .parse_html([ParseHtml::tag(r#"span[data-type="emoji"]"#)])
        .render_html(move |node, html| {
            let data_type = Map::from_iter([("data-type".into(), json!("emoji"))]);
            let attributes = merge_objects([
                &rendered_object(&html),
                &options.html_attributes,
                &data_type,
            ])?;
            let attributes = element_attrs(attributes)?;
            let name = node_attr(node, "name");
            let child = match shortcode_to_emoji(name, &options.emojis) {
                None => DomSpec::Text(format!(":{}:", name.to_js_string()?).into()),
                Some(item) => render_item(item, options.force_fallback_images),
            };
            Ok(DomSpec::element("span", attributes, vec![child]))
        })
        .render_text(move |props| {
            let name = node_attr(props.node, "name");
            let emoji = shortcode_to_emoji(name, &emojis).and_then(|item| item.emoji.as_deref());
            match emoji.filter(|emoji| !emoji.is_empty()) {
                Some(emoji) => Ok(emoji.to_owned()),
                None => Ok(format!(":{}:", name.to_js_string()?)),
            }
        })
        .render_markdown(render_markdown)
}

/// `shortcodeToEmoji(shortcode, emojis)`: the emoji of this name, or with this shortcode.
fn shortcode_to_emoji<'e>(shortcode: ValueRef, emojis: &'e [EmojiItem]) -> Option<&'e EmojiItem> {
    let shortcode = shortcode.as_str()?;
    emojis
        .iter()
        .find(|item| item.name == shortcode || item.shortcodes.iter().any(|code| code == shortcode))
}

fn render_item<'a>(item: &EmojiItem, force_fallback_images: bool) -> DomSpec<'a> {
    let emoji = item.emoji.as_deref().filter(|emoji| !emoji.is_empty());
    let has_fallback_image = item
        .fallback_image
        .as_deref()
        .is_some_and(|image| !image.is_empty());
    // `isSupported` is false: no browser draws the emoji to compare.
    let render_fallback_image = (force_fallback_images && emoji.is_none()) || has_fallback_image;
    if !render_fallback_image {
        let shortcode = item.shortcodes.first().map_or("undefined", String::as_str);
        return DomSpec::Text(match emoji {
            Some(emoji) => emoji.to_string().into(),
            None => format!(":{shortcode}:").into(),
        });
    }
    let mut image = SpecAttrs::new();
    if let Some(src) = &item.fallback_image {
        image.push("src", AttrValue::from(src.clone()));
    }
    image.push("draggable", "false".into());
    image.push("loading", "lazy".into());
    image.push("align", "absmiddle".into());
    image.push("alt", format!("{} emoji", item.name).into());
    DomSpec::element("img", image, Vec::new())
}

fn render_markdown(
    node: &Value,
    _: &dyn RenderHelpers,
    _: &RenderContext,
) -> Result<String, Error> {
    match value::optional(node.get("attrs"), "name") {
        Some(name) if js::truthy(Some(name)) => Ok(format!(":{}:", js::to_string(name)?)),
        _ => Ok(String::new()),
    }
}
