//! `@tiptap/extension-details`: `Details`, `DetailsSummary` and `DetailsContent`, each a
//! `:::name` block in Markdown. Whether a details element is open is kept only with `persist`.

use crate::markdown::specs::BlockContent;
use crate::merge_attributes::merge_configured;
use crate::{DomSpec, ExtensionAttribute, NodeExtension, ParseHtml};
use tarnish_js::json::{Map, Value, json};

#[derive(Default)]
pub struct DetailsOptions {
    /// Whether the document keeps the `open` attribute.
    pub persist: bool,
    pub html_attributes: Map,
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
                merge_configured(&html_attributes, html, [])?,
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
    pub html_attributes: Map,
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
                merge_configured(&options.html_attributes, html, [])?,
            ))
        })
        .block_markdown_spec("detailsSummary", BlockContent::Inline)
}

#[derive(Default)]
pub struct DetailsContentOptions {
    pub html_attributes: Map,
}

pub fn details_content(options: DetailsContentOptions) -> NodeExtension {
    NodeExtension::create("detailsContent")
        .content("block+")
        .defining()
        .selectable(false)
        .parse_html([ParseHtml::tag(r#"div[data-type="detailsContent"]"#)])
        .render_html(move |_, html| {
            let data_type = [("data-type", "detailsContent".into())];
            let html = merge_configured(&options.html_attributes, html, data_type)?;
            Ok(DomSpec::wrapping("div", html))
        })
        .block_markdown_spec("detailsContent", BlockContent::Block)
}
