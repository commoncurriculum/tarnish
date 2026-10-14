//! `TableHeader`.

use super::cell::cell;
use crate::NodeExtension;

#[derive(Default)]
pub struct TableHeaderOptions {
    /// `HTMLAttributes`, each a string.
    pub html_attributes: Vec<(String, String)>,
}

pub fn table_header(options: TableHeaderOptions) -> NodeExtension {
    cell("tableHeader", "th", "header_cell", options.html_attributes)
}
