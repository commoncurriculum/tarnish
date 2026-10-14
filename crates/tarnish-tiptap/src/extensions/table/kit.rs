//! `TableKit`.

use super::cell::{TableCellOptions, table_cell};
use super::header::{TableHeaderOptions, table_header};
use super::row::{TableRowOptions, table_row};
use super::{TableOptions, table};
use crate::{Extension, PlainExtension};

/// Each extension's options, `None` for `false`, which leaves it out.
pub struct TableKitOptions {
    pub table: Option<TableOptions>,
    pub table_cell: Option<TableCellOptions>,
    pub table_header: Option<TableHeaderOptions>,
    pub table_row: Option<TableRowOptions>,
}

impl Default for TableKitOptions {
    fn default() -> Self {
        TableKitOptions {
            table: Some(TableOptions::default()),
            table_cell: Some(TableCellOptions::default()),
            table_header: Some(TableHeaderOptions::default()),
            table_row: Some(TableRowOptions::default()),
        }
    }
}

/// The kit, and the extensions its `addExtensions` adds after it, as an editor flattens them.
pub fn table_kit(options: TableKitOptions) -> Vec<Extension> {
    let kit = Extension::from(PlainExtension::create("tableKit"));
    let added = [
        options.table.map(|options| Extension::from(table(options))),
        options.table_cell.map(|options| table_cell(options).into()),
        options
            .table_header
            .map(|options| table_header(options).into()),
        options.table_row.map(|options| table_row(options).into()),
    ];
    std::iter::once(kit)
        .chain(added.into_iter().flatten())
        .collect()
}
