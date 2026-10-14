//! `@tiptap/extension-list`'s `ListItem`, which `@tiptap/extension-list-item` exports.

use super::ordered_list::roman::list_marker;
use crate::markdown::{
    ParseHelpers, Parsed, RenderContext, RenderHelpers, render_nested_markdown_content,
};
use crate::merge_attributes::render_wrapping;
use crate::{HtmlAttributesOptions, NodeExtension, ParseHtml};
use tarnish_js::json::{Value, json};
use tarnish_js::{self as js, Error, value};
use tarnish_markdown::marked::Token;

pub type ListItemOptions = HtmlAttributesOptions;

pub fn list_item(options: ListItemOptions) -> NodeExtension {
    NodeExtension::create("listItem")
        .content("paragraph block*")
        .defining()
        .parse_html([ParseHtml::tag("li")])
        .render_html(render_wrapping("li", options.html_attributes, []))
        .markdown_token_name("list_item")
        .parse_markdown(parse_markdown)
        .render_markdown(render_markdown)
}

/// `isSameLineOrderedListToken(token)`: an item whose whole text is an ordered list's, as
/// `- 1. a` is.
fn is_same_line_ordered_list(token: &Token) -> bool {
    let text = token.text.as_deref().filter(|text| !text.is_empty());
    match (text, token.tokens.as_deref()) {
        (Some(text), Some([nested])) => {
            nested.kind == "list"
                && nested.list().is_some_and(|list| list.ordered)
                && *nested.raw == *text
        }
        _ => false,
    }
}

fn parse_markdown(token: &Token, helpers: &ParseHelpers) -> Result<Parsed, Error> {
    if token.kind != "list_item" {
        return Ok(Parsed::Nodes(Vec::new()));
    }
    let mut content = Vec::new();
    let tokens = token.tokens.as_deref().unwrap_or_default();
    if !tokens.is_empty() {
        if is_same_line_ordered_list(token) {
            let text = helpers.tokenize_inline(token.text())?;
            let paragraph = json!({ "type": "paragraph", "content": helpers.parse_inline(&text)? });
            return Ok(Parsed::Node(
                json!({ "type": "listItem", "content": [paragraph] }),
            ));
        }
        let first = &tokens[0];
        let inline = first.tokens.as_deref().filter(|inline| !inline.is_empty());
        let has_paragraph = tokens.iter().any(|token| token.kind == "paragraph");
        match inline.filter(|_| first.kind == "text" && !has_paragraph) {
            Some(inline) => {
                let paragraph =
                    json!({ "type": "paragraph", "content": helpers.parse_inline(inline)? });
                content.push(paragraph);
                if tokens.len() > 1 {
                    content.extend(helpers.parse_block_children(&tokens[1..])?);
                }
            }
            None => content = helpers.parse_block_children(tokens)?,
        }
    }
    if content.is_empty() {
        content.push(json!({ "type": "paragraph", "content": [] }));
    }
    Ok(Parsed::Node(
        json!({ "type": "listItem", "content": content }),
    ))
}

fn render_markdown(
    node: &Value,
    helpers: &dyn RenderHelpers,
    context: &RenderContext,
) -> Result<String, Error> {
    render_nested_markdown_content(node, helpers, || {
        let parent_type = context.parent_type.and_then(Value::as_str);
        if parent_type != Some("orderedList") {
            return Ok("- ".to_string());
        }
        let attr = |key| value::optional(context.parent_attrs, key);
        // `context.meta?.parentAttrs?.start || 1`
        let start = match attr("start") {
            Some(start) if js::truthy(Some(start)) => js::to_number(Some(start))?,
            _ => 1.0,
        };
        let index = start - 1.0 + context.index as f64;
        list_marker(attr("type"), index, ". ")
    })
}
