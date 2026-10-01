//! `@tiptap/extension-text-style`: `TextStyle` and `Color`.

use tarnish::Map as Attrs;
use tarnish::Result;
use tarnish::json::Value;
use tarnish_html::HtmlNode;
use tarnish_js::value::nullable_string;

use crate::DomSpec;
use crate::{
    ExtensionAttribute, GlobalAttributes, MarkExtension, ParseHtml, PlainExtension,
    get_style_property,
};

pub fn text_style() -> MarkExtension {
    MarkExtension::create("textStyle")
        .priority(101)
        .parse_html([ParseHtml::tag("span").not_consuming().get_attrs(|span| {
            if !span.has_attribute("style") {
                return Ok(None);
            }
            merge_nested_span_styles(span)?;
            Ok(Some(Attrs::new()))
        })])
        .render_html(|_, html| DomSpec::wrapping("span", html))
}

pub fn color() -> PlainExtension {
    PlainExtension::create("color").add_global_attributes(vec![GlobalAttributes {
        types: &["textStyle"],
        attributes: vec![
            ExtensionAttribute::new("color", Value::Null)
                .parse_html(|element| {
                    let value = get_style_property(element, "color")
                        .unwrap_or_else(|| element.style_value("color"));
                    Some(Value::String(value.replace(['\'', '"'], "")))
                })
                .render_html(|attributes, rendered| match attributes.get("color") {
                    Some(color) if color.truthy() => {
                        rendered.merge("style", format!("color: {}", color.to_js_string()?))
                    }
                    _ => Ok(()),
                }),
        ],
    }])
}

/// `mergeNestedSpanStyles`: each span inside `element`, down to the first span on each path,
/// gets the style of the closest span above it put ahead of its own.
fn merge_nested_span_styles(element: &HtmlNode) -> Result<()> {
    if element.children().is_empty() {
        return Ok(());
    }
    for span in find_child_spans(element, 0) {
        let child_style = span.attribute("style");
        let parent_style = span
            .parent_element()
            .map(|parent| parent.closest("span"))
            .transpose()?
            .flatten()
            .map(|parent| parent.attribute("style"));
        let style = format!(
            "{};{}",
            nullable_string(&parent_style),
            nullable_string(&Some(child_style))
        );
        span.set_attribute("style", &style)?;
    }
    Ok(())
}

fn find_child_spans(element: &HtmlNode, depth: usize) -> Vec<HtmlNode> {
    let children = element.children();
    if children.is_empty() || depth > 20 {
        return Vec::new();
    }
    let mut spans = Vec::new();
    for child in children {
        if child.is_tag("span") {
            spans.push(child);
        } else if !child.children().is_empty() {
            spans.extend(find_child_spans(&child, depth + 1));
        }
    }
    spans
}
