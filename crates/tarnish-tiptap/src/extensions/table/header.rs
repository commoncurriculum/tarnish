//! `TableHeader`.

use super::cell::cell;
use crate::NodeExtension;
use tarnish_js::json::Map;

#[derive(Default)]
pub struct TableHeaderOptions {
    pub html_attributes: Map,
}

pub fn table_header(options: TableHeaderOptions) -> NodeExtension {
    cell("tableHeader", "th", "header_cell", options.html_attributes)
}
