//! `@tiptap/extension-table`: Table, TableRow, TableCell and TableHeader, which
//! `@tiptap/extension-table-row`, `-table-cell` and `-table-header` export again, and TableKit.
//! Its commands, keyboard shortcuts and node view, and the editing and column-resizing plugins
//! of prosemirror-tables it adds, are the editor's.

mod cell;
mod col_group;
mod header;
mod kit;
mod markdown;
mod row;
mod utils;

pub use cell::{TableCellOptions, table_cell};
pub use header::{TableHeaderOptions, table_header};
pub use kit::{TableKitOptions, table_kit};
pub use row::{TableRowOptions, table_row};

use tarnish_js::Error;
use tarnish_js::json::{Map, json};
use tarnish_js::value::{self, Nullish};
use tarnish_markdown::marked::{Token, TokenData};

use crate::markdown::{ParseHelpers, Parsed};
use crate::merge_attributes::merge_configured;
use crate::{AttrValue, DomSpec, NodeExtension, ParseHtml};
use col_group::create_col_group;
use markdown::{render_table_to_markdown, tokenize, tokenizer_start};
use utils::normalize_table_cell_align;

/// The options that change what a table is or renders. `resizable`, `handleWidth`, `View`,
/// `lastColumnResizable` and `allowTableNodeSelection` change only an editor's plugins and node
/// view.
pub struct TableOptions {
    pub html_attributes: Map,
    /// `renderWrapper`: whether the table renders in a `div.tableWrapper`.
    pub render_wrapper: bool,
    /// `cellMinWidth`: the width in pixels of a column with none of its own, and the least of
    /// one with.
    pub cell_min_width: f64,
}

impl Default for TableOptions {
    fn default() -> Self {
        TableOptions {
            html_attributes: Map::new(),
            render_wrapper: false,
            cell_min_width: 25.0,
        }
    }
}

/// `Table`.
pub fn table(options: TableOptions) -> NodeExtension {
    let TableOptions {
        html_attributes,
        render_wrapper,
        cell_min_width,
    } = options;
    NodeExtension::create("table")
        .content("tableRow+")
        .field("tableRole", "table")
        .isolating()
        .group("block")
        .parse_html([ParseHtml::tag("table")])
        .render_html(move |node, html| {
            let col_group = create_col_group(node, cell_min_width)?;
            let style = match html.get("style").filter(|style| style.truthy()) {
                Some(user_styles) => user_styles.clone(),
                None => AttrValue::from(match &col_group {
                    Some(col_group) if !col_group.table_width.is_empty() => {
                        format!("width: {}", col_group.table_width)
                    }
                    Some(col_group) => format!("min-width: {}", col_group.table_min_width),
                    None => "min-width: undefined".to_owned(),
                }),
            };
            let attrs = merge_configured(&html_attributes, html, [("style", style)])?;
            // A table without rows renders an `undefined` colgroup, which `renderSpec` reads.
            let Some(col_group) = col_group else {
                return Err(value::cannot_read(Nullish::Undefined, "nodeType"));
            };
            let tbody = DomSpec::wrapping("tbody", Default::default());
            let table = DomSpec::element("table", attrs, vec![col_group.colgroup, tbody]);
            Ok(match render_wrapper {
                true => {
                    let class = [("class", AttrValue::from("tableWrapper"))].into();
                    DomSpec::element("div", class, vec![table])
                }
                false => table,
            })
        })
        .parse_markdown(parse_markdown)
        .render_markdown(|node, helpers, _| render_table_to_markdown(node, helpers))
        .markdown_block_tokenizer(tokenizer_start, tokenize)
        .extend_node_schema(|extension| {
            let mut fields = Map::new();
            if let Some(role) = extension.fields.get("tableRole") {
                fields.insert("tableRole".into(), role.clone());
            }
            fields
        })
}

/// `parseMarkdown`: a row of header cells, then a row of cells for each of marked's rows, each
/// cell a paragraph aligned as its column.
fn parse_markdown(token: &Token, helpers: &ParseHelpers) -> Result<Parsed, Error> {
    let mut rows = Vec::new();
    if let TokenData::Table(table) = &token.data {
        let cell = |kind: &str, index: usize, tokens: &[Token]| -> Result<_, Error> {
            let align = table.align.get(index).copied().flatten();
            let align = normalize_table_cell_align(align.map(|align| align.as_str()));
            let mut attrs = Map::new();
            if let Some(align) = align {
                attrs.insert("align".into(), json!(align));
            }
            let content = helpers.parse_inline(tokens)?;
            let paragraph = json!({ "type": "paragraph", "content": content });
            Ok(helpers.create_node(kind, Some(attrs), Some(vec![paragraph])))
        };
        let header = table.header.iter().enumerate();
        let header_cells = header
            .map(|(index, header)| cell("tableHeader", index, &header.tokens))
            .collect::<Result<_, Error>>()?;
        rows.push(helpers.create_node("tableRow", None, Some(header_cells)));
        for row in &table.rows {
            let body_cells = row.iter().enumerate();
            let body_cells = body_cells
                .map(|(index, body)| cell("tableCell", index, &body.tokens))
                .collect::<Result<_, Error>>()?;
            rows.push(helpers.create_node("tableRow", None, Some(body_cells)));
        }
    }
    Ok(Parsed::Node(helpers.create_node("table", None, Some(rows))))
}
