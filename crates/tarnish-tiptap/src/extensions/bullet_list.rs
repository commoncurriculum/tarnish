//! `@tiptap/extension-list`'s `BulletList`, which `@tiptap/extension-bullet-list` exports.

use crate::markdown::{ParseHelpers, Parsed, RenderContext, RenderHelpers};
use crate::merge_attributes::render_wrapping;
use crate::{NodeExtension, ParseHtml};
use tarnish_js::Error;
use tarnish_js::json::{Map, Value, json};
use tarnish_markdown::marked::Token;

pub struct BulletListOptions {
    /// The node type of the list's items.
    pub item_type_name: &'static str,
    pub html_attributes: Map,
}

impl Default for BulletListOptions {
    fn default() -> Self {
        BulletListOptions {
            item_type_name: "listItem",
            html_attributes: Map::new(),
        }
    }
}

pub fn bullet_list(options: BulletListOptions) -> NodeExtension {
    NodeExtension::create("bulletList")
        .group("block list")
        .content(format!("{}+", options.item_type_name))
        .parse_html([ParseHtml::tag("ul")])
        .render_html(render_wrapping("ul", options.html_attributes, []))
        .markdown_token_name("list")
        .parse_markdown(parse_markdown)
        .render_markdown(render_list)
        .indents_content()
}

fn parse_markdown(token: &Token, helpers: &ParseHelpers) -> Result<Parsed, Error> {
    if token.kind != "list" || token.list().is_some_and(|list| list.ordered) {
        return Ok(Parsed::Nodes(Vec::new()));
    }
    let items = token.list().map_or(&[][..], |list| &list.items);
    let content = helpers.parse_children(items)?;
    Ok(Parsed::Node(
        json!({ "type": "bulletList", "content": content }),
    ))
}

/// A list's `renderMarkdown`: its items, a line apart.
pub(super) fn render_list(
    node: &Value,
    helpers: &dyn RenderHelpers,
    _: &RenderContext,
) -> Result<String, Error> {
    match node.get("content") {
        Some(content) if tarnish_js::truthy(Some(content)) => {
            helpers.render_children(content, "\n")
        }
        _ => Ok(String::new()),
    }
}
