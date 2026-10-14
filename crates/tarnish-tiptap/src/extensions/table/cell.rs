//! `TableCell`, and the TableHeader it differs from in its name, tag and role.

use super::utils::{cell_attributes, cell_parse_html};
use crate::merge_attributes::merge_configured;
use crate::{DomSpec, NodeExtension};
use tarnish_js::json::Map;

#[derive(Default)]
pub struct TableCellOptions {
    pub html_attributes: Map,
}

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
        .render_html(move |_, html| {
            let attrs = merge_configured(&html_attributes, html, [])?;
            Ok(DomSpec::wrapping(tag, attrs))
        })
}
