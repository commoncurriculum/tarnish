//! `TableRow`.

use crate::merge_attributes::merge_configured;
use crate::{DomSpec, NodeExtension, ParseHtml};
use tarnish_js::json::Map;

#[derive(Default)]
pub struct TableRowOptions {
    pub html_attributes: Map,
}

pub fn table_row(options: TableRowOptions) -> NodeExtension {
    let html_attributes = options.html_attributes;
    NodeExtension::create("tableRow")
        .content("(tableCell | tableHeader)*")
        .field("tableRole", "row")
        .parse_html([ParseHtml::tag("tr")])
        .render_html(move |_, html| {
            let attrs = merge_configured(&html_attributes, html, [])?;
            Ok(DomSpec::wrapping("tr", attrs))
        })
}
