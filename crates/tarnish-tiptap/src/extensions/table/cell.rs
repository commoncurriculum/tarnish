//! `TableCell`, and the TableHeader it differs from in its name, tag and role.

use super::utils::{cell_attributes, cell_parse_html};
use crate::merge_attributes::render_wrapping;
use crate::{HtmlAttributesOptions, NodeExtension};
use tarnish_js::json::Map;

pub type TableCellOptions = HtmlAttributesOptions;

pub fn table_cell(options: TableCellOptions) -> NodeExtension {
    cell("tableCell", "td", "cell", options.html_attributes)
}

/// A cell named `name`, of the element `tag` and the `tableRole` `role`.
pub(super) fn cell(
    name: &'static str,
    tag: &'static str,
    role: &'static str,
    html_attributes: Map,
) -> NodeExtension {
    NodeExtension::create(name)
        .content("block+")
        .add_attributes(cell_attributes())
        .field("tableRole", role)
        .isolating()
        .parse_html(cell_parse_html(tag, name))
        .render_html(render_wrapping(tag, html_attributes, []))
}
