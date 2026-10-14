//! `@tiptap/extension-list`'s `TaskItem`, which `@tiptap/extension-task-item` exports: its
//! checkbox, which only its node view and accessibility label change, renders as the HTML does.

use crate::markdown::{
    ParseHelpers, Parsed, RenderContext, RenderHelpers, render_nested_markdown_content,
};
use crate::merge_attributes::merge_configured;
use crate::{AttrValue, DomSpec, ExtensionAttribute, NodeExtension, ParseHtml};
use crate::{SpecAttrs, node_attr};
use tarnish_js::json::{Map, Value, json};
use tarnish_js::{self as js, Error, utf16, value};
use tarnish_markdown::marked::{Token, TokenData};

#[derive(Default)]
pub struct TaskItemOptions {
    /// Whether an item holds blocks after its paragraph, task lists among them, or only
    /// paragraphs.
    pub nested: bool,
    pub html_attributes: Map,
}

pub fn task_item(options: TaskItemOptions) -> NodeExtension {
    let attributes = options.html_attributes;
    NodeExtension::create("taskItem")
        .content(match options.nested {
            true => "paragraph block*",
            false => "paragraph+",
        })
        .defining()
        .add_attributes(vec![
            ExtensionAttribute::new("checked", json!(false))
                .parse_html(|element| {
                    let checked = element.attribute("data-checked");
                    Some(Value::Bool(matches!(checked.as_deref(), Some("" | "true"))))
                })
                .render_html(|attributes, rendered| {
                    rendered.merge("data-checked", attributes.get("checked").expect("checked"))
                }),
        ])
        .parse_html([ParseHtml::tag(r#"li[data-type="taskItem"]"#)
            .priority(51)
            .content_element(|element| {
                Ok(element
                    .query_selector("div")?
                    .unwrap_or_else(|| element.clone()))
            })])
        .render_html(move |node, html| {
            let data_type = [("data-type", AttrValue::from("taskItem"))];
            let html = merge_configured(&attributes, html, data_type)?;
            let mut checkbox = SpecAttrs::from([("type", AttrValue::from("checkbox"))]);
            // `checked: node.attrs.checked ? 'checked' : null`
            if node_attr(node, "checked").truthy() {
                checkbox.push("checked", "checked".into());
            }
            let label = DomSpec::element(
                "label",
                SpecAttrs::new(),
                vec![
                    DomSpec::element("input", checkbox, Vec::new()),
                    DomSpec::element("span", SpecAttrs::new(), Vec::new()),
                ],
            );
            let content = DomSpec::wrapping("div", SpecAttrs::new());
            Ok(DomSpec::element("li", html, vec![label, content]))
        })
        .parse_markdown(parse_markdown)
        .render_markdown(render_markdown)
}

fn parse_markdown(token: &Token, helpers: &ParseHelpers) -> Result<Parsed, Error> {
    let paragraph = |content| helpers.create_node("paragraph", None, Some(content));
    let mut content = Vec::new();
    match token.tokens.as_deref().filter(|tokens| !tokens.is_empty()) {
        Some(tokens) => content.push(paragraph(helpers.parse_inline(tokens)?)),
        None if !token.text().is_empty() => {
            let mut text = Map::new();
            text.insert("text".into(), json!(utf16::to_string(token.text())));
            content.push(paragraph(vec![helpers.create_node(
                "text",
                Some(text),
                None,
            )]));
        }
        None => content.push(paragraph(Vec::new())),
    }
    let nested = token.child_tokens("nestedTokens").unwrap_or_default();
    if !nested.is_empty() {
        content.extend(helpers.parse_children(nested)?);
    }
    let checked = matches!(
        token.data,
        TokenData::ListItem {
            checked: Some(true),
            ..
        }
    );
    let mut attrs = Map::new();
    attrs.insert("checked".into(), Value::Bool(checked));
    Ok(Parsed::Node(helpers.create_node(
        "taskItem",
        Some(attrs),
        Some(content),
    )))
}

fn render_markdown(
    node: &Value,
    helpers: &dyn RenderHelpers,
    _: &RenderContext,
) -> Result<String, Error> {
    let checked = js::truthy(value::optional(node.get("attrs"), "checked"));
    let prefix = format!("- [{}] ", if checked { 'x' } else { ' ' });
    render_nested_markdown_content(node, helpers, || Ok(prefix))
}
