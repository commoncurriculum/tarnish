//! `TableHeader`.

use super::cell::cell;
use crate::{HtmlAttributesOptions, NodeExtension};

pub type TableHeaderOptions = HtmlAttributesOptions;

pub fn table_header(options: TableHeaderOptions) -> NodeExtension {
    cell("tableHeader", "th", "header_cell", options.html_attributes)
}
