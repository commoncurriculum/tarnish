//! `TableRow`.

use crate::{DomSpec, NodeExtension, ParseHtml, merge_html_attributes};

#[derive(Default)]
pub struct TableRowOptions {
    /// `HTMLAttributes`, each a string.
    pub html_attributes: Vec<(String, String)>,
}

pub fn table_row(options: TableRowOptions) -> NodeExtension {
    let html_attributes = options.html_attributes;
    NodeExtension::create("tableRow")
        .content("(tableCell | tableHeader)*")
        .field("tableRole", "row")
        .parse_html([ParseHtml::tag("tr")])
        .render_html(move |_, html| {
            let attrs = merge_html_attributes(&html_attributes, html)?;
            Ok(DomSpec::wrapping("tr", attrs))
        })
}
