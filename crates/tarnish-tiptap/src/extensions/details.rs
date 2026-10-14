//! `@tiptap/extension-details`: `Details`, `DetailsSummary` and `DetailsContent`, each a
//! `:::name` block in Markdown. Whether a details element is open is kept only with `persist`.

use crate::markdown::specs::BlockContent;
use crate::{
    DomSpec, ExtensionAttribute, NodeExtension, ParseHtml, merge_attributes, merge_html_attributes,
};
use tarnish_js::json::{Value, json};

#[derive(Default)]
pub struct DetailsOptions {
    /// Whether the document keeps the `open` attribute.
    pub persist: bool,
    pub html_attributes: Vec<(String, String)>,
}

pub fn details(options: DetailsOptions) -> NodeExtension {
    let attributes = match options.persist {
        true => vec![open()],
        false => Vec::new(),
    };
    let html_attributes = options.html_attributes;
    NodeExtension::create("details")
        .content("detailsSummary detailsContent")
        .group("block")
        .defining()
        .isolating()
        .add_attributes(attributes)
        .parse_html([ParseHtml::tag("details")])
        .render_html(move |_, html| {
            Ok(DomSpec::wrapping(
                "details",
                merge_html_attributes(&html_attributes, html)?,
            ))
        })
        .block_markdown_spec("details", BlockContent::Block)
}

fn open() -> ExtensionAttribute {
    ExtensionAttribute::new("open", json!(false))
        .parse_html(|element| Some(Value::Bool(element.has_attribute("open"))))
        .render_html(|attributes, rendered| match attributes.get("open") {
            Some(open) if open.truthy() => rendered.merge("open", ""),
            _ => Ok(()),
        })
}

#[derive(Default)]
pub struct DetailsSummaryOptions {
    pub html_attributes: Vec<(String, String)>,
}

pub fn details_summary(options: DetailsSummaryOptions) -> NodeExtension {
    NodeExtension::create("detailsSummary")
        .content("text*")
        .defining()
        .selectable(false)
        .isolating()
        .parse_html([ParseHtml::tag("summary")])
        .render_html(move |_, html| {
            Ok(DomSpec::wrapping(
                "summary",
                merge_html_attributes(&options.html_attributes, html)?,
            ))
        })
        .block_markdown_spec("detailsSummary", BlockContent::Inline)
}

#[derive(Default)]
pub struct DetailsContentOptions {
    pub html_attributes: Vec<(String, String)>,
}

pub fn details_content(options: DetailsContentOptions) -> NodeExtension {
    NodeExtension::create("detailsContent")
        .content("block+")
        .defining()
        .selectable(false)
        .parse_html([ParseHtml::tag(r#"div[data-type="detailsContent"]"#)])
        .render_html(move |_, html| {
            let html = merge_html_attributes(&options.html_attributes, html)?;
            let html = merge_attributes(html, [("data-type", "detailsContent".into())])?;
            Ok(DomSpec::wrapping("div", html))
        })
        .block_markdown_spec("detailsContent", BlockContent::Block)
}
