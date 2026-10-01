//! `@tiptap/extension-heading`.

use crate::DomSpec;
use crate::markdown::{ParseHelpers, Parsed, RenderContext, RenderHelpers};
use crate::{ExtensionAttribute, NodeExtension, ParseHtml};
use tarnish_js::json::{Value, json, object};
use tarnish_js::{self as js, Error, value};
use tarnish_markdown::marked::Token;

pub struct HeadingOptions {
    pub levels: &'static [u8],
}

impl Default for HeadingOptions {
    fn default() -> Self {
        HeadingOptions {
            levels: &[1, 2, 3, 4, 5, 6],
        }
    }
}

pub fn heading(options: HeadingOptions) -> NodeExtension {
    let levels = options.levels;
    NodeExtension::create("heading")
        .content("inline*")
        .group("block")
        .add_attributes(vec![
            ExtensionAttribute::new("level", json!(1)).not_rendered(),
        ])
        .parse_html(
            levels.iter().map(|level| {
                ParseHtml::tag(format!("h{level}")).attrs(object!({ "level": level }))
            }),
        )
        .render_html(move |node, html| {
            let level = node
                .attrs_view()
                .get("level")
                .and_then(|level| level.as_f64())
                .filter(|level| levels.iter().any(|allowed| f64::from(*allowed) == *level))
                .map_or(levels[0], |level| level as u8);
            Ok(DomSpec::wrapping(TAGS[usize::from(level) - 1], html))
        })
        .parse_markdown(parse_markdown)
        .render_markdown(render_markdown)
}

const TAGS: [&str; 6] = ["h1", "h2", "h3", "h4", "h5", "h6"];

fn parse_markdown(token: &Token, helpers: &ParseHelpers) -> Result<Parsed, Error> {
    let level = token.depth().filter(|&depth| depth != 0).unwrap_or(1);
    let content = helpers.parse_inline(token.tokens.as_deref().unwrap_or_default())?;
    Ok(Parsed::Node(helpers.create_node(
        "heading",
        Some(object!({ "level": level })),
        Some(content),
    )))
}

fn render_markdown(
    node: &Value,
    helpers: &dyn RenderHelpers,
    _: &RenderContext,
) -> Result<String, Error> {
    // `node.attrs?.level ? parseInt(node.attrs.level, 10) : 1`
    let level = match value::optional(node.get("attrs"), "level") {
        Some(level) if js::truthy(Some(level)) => js::parse_int_radix(&js::to_string(level)?, 10),
        _ => 1.0,
    };
    let heading_chars = js::repeat("#", level)?;
    match node.get("content") {
        Some(content) if js::truthy(Some(content)) => Ok(format!(
            "{heading_chars} {}",
            helpers.render_children(content, "")?
        )),
        _ => Ok(String::new()),
    }
}
