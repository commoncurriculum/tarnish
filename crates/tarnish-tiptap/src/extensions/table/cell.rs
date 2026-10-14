//! `TableCell`, and the TableHeader it differs from in its name, tag and role.

use super::utils::{cell_attributes, cell_parse_html};
use crate::{DomSpec, NodeExtension, merge_html_attributes};

#[derive(Default)]
pub struct TableCellOptions {
    /// `HTMLAttributes`, each a string.
    pub html_attributes: Vec<(String, String)>,
}

pub fn table_cell(options: TableCellOptions) -> NodeExtension {
    cell("tableCell", "td", "cell", options.html_attributes)
}

/// A cell named `name`, of the element `tag` and the `tableRole` `role`.
pub(super) fn cell(
    name: &'static str,
    tag: &'static str,
    role: &'static str,
    html_attributes: Vec<(String, String)>,
) -> NodeExtension {
    NodeExtension::create(name)
        .content("block+")
        .add_attributes(cell_attributes())
        .field("tableRole", role)
        .isolating()
        .parse_html(cell_parse_html(tag, name))
        .render_html(move |_, html| {
            let attrs = merge_html_attributes(&html_attributes, html)?;
            Ok(DomSpec::wrapping(tag, attrs))
        })
}
