//! `TableRow`.

use crate::merge_attributes::render_wrapping;
use crate::{HtmlAttributesOptions, NodeExtension, ParseHtml};

pub type TableRowOptions = HtmlAttributesOptions;

pub fn table_row(options: TableRowOptions) -> NodeExtension {
    NodeExtension::create("tableRow")
        .content("(tableCell | tableHeader)*")
        .field("tableRole", "row")
        .parse_html([ParseHtml::tag("tr")])
        .render_html(render_wrapping("tr", options.html_attributes, []))
}
