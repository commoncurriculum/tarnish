//! `@tiptap/extension-details`: `Details`, `DetailsSummary` and `DetailsContent`, each a
//! `:::name` block in Markdown. Whether a details element is open is kept only with `persist`.

use crate::markdown::specs::BlockContent;
use crate::merge_attributes::render_wrapping;
use crate::{ExtensionAttribute, HtmlAttributesOptions, NodeExtension, ParseHtml};
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
    NodeExtension::create("details")
        .content("detailsSummary detailsContent")
        .group("block")
        .defining()
        .isolating()
        .add_attributes(attributes)
        .parse_html([ParseHtml::tag("details")])
        .render_html(render_wrapping("details", options.html_attributes, []))
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

pub type DetailsSummaryOptions = HtmlAttributesOptions;

pub fn details_summary(options: DetailsSummaryOptions) -> NodeExtension {
    NodeExtension::create("detailsSummary")
        .content("text*")
        .defining()
        .selectable(false)
        .isolating()
        .parse_html([ParseHtml::tag("summary")])
        .render_html(render_wrapping("summary", options.html_attributes, []))
        .block_markdown_spec("detailsSummary", BlockContent::Inline)
}

pub type DetailsContentOptions = HtmlAttributesOptions;

pub fn details_content(options: DetailsContentOptions) -> NodeExtension {
    let data_type = [("data-type", "detailsContent")];
    NodeExtension::create("detailsContent")
        .content("block+")
        .defining()
        .selectable(false)
        .parse_html([ParseHtml::tag(r#"div[data-type="detailsContent"]"#)])
        .render_html(render_wrapping("div", options.html_attributes, data_type))
        .block_markdown_spec("detailsContent", BlockContent::Block)
}
