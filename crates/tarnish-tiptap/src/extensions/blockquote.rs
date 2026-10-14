//! `@tiptap/extension-blockquote`.

use crate::markdown::{ParseHelpers, Parsed, RenderContext, RenderHelpers};
use crate::{
    DomSpec, NodeExtension, ParseHtml, element_attrs, merge_html_attributes, rendered_object,
};
use tarnish_js::json::Value;
use tarnish_js::{self as js, Error, value};
use tarnish_markdown::marked::Token;

#[derive(Default)]
pub struct BlockquoteOptions {
    pub html_attributes: Vec<(String, String)>,
}

pub fn blockquote(options: BlockquoteOptions) -> NodeExtension {
    NodeExtension::create("blockquote")
        .content("block+")
        .group("block")
        .defining()
        .parse_html([ParseHtml::tag("blockquote")])
        .render_html(move |_, html| {
            let mut html = merge_html_attributes(&options.html_attributes, html)?;
            // The JSX runtime takes `children` for the element's content.
            if html.get("children").is_some() {
                let mut object = rendered_object(&html);
                object.shift_remove("children");
                html = element_attrs(object)?;
            }
            Ok(DomSpec::wrapping("blockquote", html))
        })
        .parse_markdown(parse_markdown)
        .render_markdown(render_markdown)
}

fn parse_markdown(token: &Token, helpers: &ParseHelpers) -> Result<Parsed, Error> {
    let content = helpers.parse_block_children(token.tokens.as_deref().unwrap_or_default())?;
    Ok(Parsed::Node(helpers.create_node(
        "blockquote",
        None,
        Some(content),
    )))
}

/// Each child's lines behind `> `, a blank line behind `>` alone, and a `>` line between
/// children: a blockquote inside adds its own.
fn render_markdown(
    node: &Value,
    helpers: &dyn RenderHelpers,
    _: &RenderContext,
) -> Result<String, Error> {
    let content = node.get("content");
    if !js::truthy(content) {
        return Ok(String::new());
    }
    let children = value::array_method(content, "node.content.forEach")?;
    let mut result = Vec::with_capacity(children.len());
    for (index, child) in children.iter().enumerate() {
        let child_content = helpers.render_child(child, index)?;
        let lines: Vec<String> = child_content
            .split('\n')
            .map(|line| match js::trim(line).is_empty() {
                true => ">".to_string(),
                false => format!("> {line}"),
            })
            .collect();
        result.push(lines.join("\n"));
    }
    Ok(result.join("\n>\n"))
}
