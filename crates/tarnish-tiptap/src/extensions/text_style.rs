//! `@tiptap/extension-text-style`: `TextStyle`, the extensions that give it the styles it spans,
//! `BackgroundColor`, `Color`, `FontFamily`, `FontSize` and `LineHeight`, and `TextStyleKit`.
//! `@tiptap/extension-color` and `@tiptap/extension-font-family` are `Color` and `FontFamily`.

use tarnish_html::HtmlNode;
use tarnish_js::Result;
use tarnish_js::json::{Map, Value};
use tarnish_js::value::nullable_string;

use crate::merge_attributes::render_wrapping;
use crate::{
    Extension, ExtensionAttribute, GlobalAttributes, GlobalTypes, MarkExtension, ParseHtml,
    PlainExtension, get_style_property,
};

pub struct TextStyleOptions {
    pub html_attributes: Map,
    /// Whether a span's nested spans get its style ahead of theirs as it's parsed.
    pub merge_nested_span_styles: bool,
}

impl Default for TextStyleOptions {
    fn default() -> Self {
        TextStyleOptions {
            html_attributes: Map::new(),
            merge_nested_span_styles: true,
        }
    }
}

pub fn text_style(options: TextStyleOptions) -> MarkExtension {
    let merge = options.merge_nested_span_styles;
    MarkExtension::create("textStyle")
        .priority(101)
        .parse_html([ParseHtml::tag("span")
            .not_consuming()
            .get_attrs(move |span| {
                if !span.has_attribute("style") {
                    return Ok(None);
                }
                if merge {
                    merge_nested_span_styles(span)?;
                }
                Ok(Some(Map::new()))
            })])
        .render_html(render_wrapping("span", options.html_attributes, []))
}

/// The options of each extension that gives types a style: the types it gives it to.
pub struct StyleOptions {
    pub types: &'static [&'static str],
}

impl Default for StyleOptions {
    fn default() -> Self {
        StyleOptions {
            types: &["textStyle"],
        }
    }
}

pub type BackgroundColorOptions = StyleOptions;
pub type ColorOptions = StyleOptions;
pub type FontFamilyOptions = StyleOptions;
pub type FontSizeOptions = StyleOptions;
pub type LineHeightOptions = StyleOptions;

pub fn background_color(options: BackgroundColorOptions) -> PlainExtension {
    styled("backgroundColor", "background-color", true, options)
}

pub fn color(options: ColorOptions) -> PlainExtension {
    styled("color", "color", true, options)
}

pub fn font_family(options: FontFamilyOptions) -> PlainExtension {
    styled("fontFamily", "font-family", false, options)
}

pub fn font_size(options: FontSizeOptions) -> PlainExtension {
    styled("fontSize", "font-size", false, options)
}

pub fn line_height(options: LineHeightOptions) -> PlainExtension {
    styled("lineHeight", "line-height", false, options)
}

/// An extension named `name` that gives its types an attribute of the same name: the CSS
/// `property` of the element's inline style, without its quotes where `unquoted`, and the style
/// it renders.
fn styled(
    name: &'static str,
    property: &'static str,
    unquoted: bool,
    options: StyleOptions,
) -> PlainExtension {
    let attribute = ExtensionAttribute::new(name, Value::Null)
        .parse_html(move |element| {
            let value = get_style_property(element, property)
                .unwrap_or_else(|| element.style_value(property));
            Some(Value::String(if unquoted {
                value.replace(['\'', '"'], "")
            } else {
                value
            }))
        })
        .render_html(move |attributes, rendered| match attributes.get(name) {
            Some(value) if value.truthy() => {
                rendered.merge("style", format!("{property}: {}", value.to_js_string()?))
            }
            _ => Ok(()),
        });
    PlainExtension::create(name).add_global_attributes(vec![GlobalAttributes {
        types: GlobalTypes::Named(options.types),
        attributes: vec![attribute],
    }])
}

/// Which of its extensions `TextStyleKit` adds, with their options: `None` for `false`.
pub struct TextStyleKitOptions {
    pub background_color: Option<BackgroundColorOptions>,
    pub color: Option<ColorOptions>,
    pub font_family: Option<FontFamilyOptions>,
    pub font_size: Option<FontSizeOptions>,
    pub line_height: Option<LineHeightOptions>,
    pub text_style: Option<TextStyleOptions>,
}

impl Default for TextStyleKitOptions {
    fn default() -> Self {
        TextStyleKitOptions {
            background_color: Some(StyleOptions::default()),
            color: Some(StyleOptions::default()),
            font_family: Some(StyleOptions::default()),
            font_size: Some(StyleOptions::default()),
            line_height: Some(StyleOptions::default()),
            text_style: Some(TextStyleOptions::default()),
        }
    }
}

/// `TextStyleKit`, and the extensions it adds after it, as `flattenExtensions` puts them.
pub fn text_style_kit(options: TextStyleKitOptions) -> Vec<Extension> {
    let styles = [
        options.background_color.map(background_color),
        options.color.map(color),
        options.font_family.map(font_family),
        options.font_size.map(font_size),
        options.line_height.map(line_height),
    ];
    let kit = PlainExtension::create("textStyleKit").into();
    let styles = styles.into_iter().flatten().map(Extension::from);
    let text_style = options.text_style.map(|options| text_style(options).into());
    std::iter::once(kit)
        .chain(styles)
        .chain(text_style)
        .collect()
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

// bounded: it stops past depth 20
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
