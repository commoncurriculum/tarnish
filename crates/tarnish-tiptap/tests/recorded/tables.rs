//! `fixtures/tiptap-tables.json`, of @tiptap/extension-table's Table, TableRow, TableCell and
//! TableHeader, and tables nested in cells far deeper than a small stack could recurse through.

use tarnish::json::{Map, Value, json, object};
use tarnish_html::HtmlDom;
use tarnish_js::utf16;
use tarnish_tiptap::extensions::bold::bold;
use tarnish_tiptap::extensions::document::document;
use tarnish_tiptap::extensions::hard_break::hard_break;
use tarnish_tiptap::extensions::heading::{HeadingOptions, heading};
use tarnish_tiptap::extensions::italic::italic;
use tarnish_tiptap::extensions::paragraph::paragraph;
use tarnish_tiptap::extensions::table::{
    TableCellOptions, TableHeaderOptions, TableKitOptions, TableOptions, TableRowOptions, table,
    table_cell, table_header, table_kit, table_row,
};
use tarnish_tiptap::extensions::text::text;
use tarnish_tiptap::{
    DomSpec, Extension, ExtensionAttribute, GlobalAttributes, GlobalTypes, MarkExtension,
    ParseHtml, PlainExtension,
};

use crate::own::{self, Configuration, Nested, Replay};

/// An application's code mark, which parses marked's code spans.
fn code() -> MarkExtension {
    MarkExtension::create("code")
        .code()
        .parse_html([ParseHtml::tag("code")])
        .render_html(|_, html| Ok(DomSpec::wrapping("code", html)))
        .markdown_token_name("codespan")
        .parse_markdown(|token, helpers| {
            let text = json!({ "type": "text", "text": utf16::to_string(token.text()) });
            Ok(helpers.apply_mark("code", vec![text], None))
        })
        .render_markdown(|node, helpers, _| {
            Ok(["`", &helpers.render_children(node, "")?, "`"].concat())
        })
}

fn inline() -> Vec<Extension> {
    vec![
        document().into(),
        paragraph(Default::default()).into(),
        text().into(),
        hard_break(Default::default()).into(),
        bold(Default::default()).into(),
        italic(Default::default()).into(),
        heading(HeadingOptions::default()).into(),
        code().into(),
    ]
}

/// The inline extensions, then the table's four with these options.
fn tables(
    table_options: TableOptions,
    row: TableRowOptions,
    cell: TableCellOptions,
    header: TableHeaderOptions,
) -> Vec<Extension> {
    let mut extensions = inline();
    extensions.push(table(table_options).into());
    extensions.push(table_row(row).into());
    extensions.push(table_cell(cell).into());
    extensions.push(table_header(header).into());
    extensions
}

fn with_table(options: TableOptions) -> Vec<Extension> {
    tables(
        options,
        Default::default(),
        Default::default(),
        Default::default(),
    )
}

/// An application's `style` attribute on tables and cells.
fn style() -> PlainExtension {
    let style = ExtensionAttribute::new("style", Value::Null)
        .parse_html(|element| element.attribute("style").map(Value::String))
        .render_html(|attributes, rendered| match attributes.get("style") {
            Some(style) if style.truthy() => rendered.merge("style", style),
            _ => Ok(()),
        });
    PlainExtension::create("style").add_global_attributes(vec![GlobalAttributes {
        types: GlobalTypes::Named(&["table", "tableCell", "tableHeader"]),
        attributes: vec![style],
    }])
}

fn configuration(name: &str) -> Configuration {
    match name {
        "tables" => with_table(TableOptions::default()),
        "attributes" => tables(
            TableOptions {
                html_attributes: object!({"class": "t", "style": "color: red; min-width: 1px"}),
                ..TableOptions::default()
            },
            TableRowOptions {
                html_attributes: object!({"class": "r"}),
            },
            TableCellOptions {
                html_attributes: object!({"class": "c", "style": "padding: 1px"}),
            },
            TableHeaderOptions {
                html_attributes: object!({"class": "h", "data-x": "y"}),
            },
        ),
        "attributeValues" => tables(
            TableOptions {
                html_attributes: own::other_attributes(),
                ..TableOptions::default()
            },
            TableRowOptions {
                html_attributes: own::other_attributes(),
            },
            TableCellOptions {
                html_attributes: own::other_attributes(),
            },
            TableHeaderOptions {
                html_attributes: own::other_attributes(),
            },
        ),
        "wrapper" => with_table(TableOptions {
            render_wrapper: true,
            ..TableOptions::default()
        }),
        "minWidth" => with_table(TableOptions {
            cell_min_width: 12.5,
            ..TableOptions::default()
        }),
        "zero" => with_table(TableOptions {
            cell_min_width: 0.0,
            ..TableOptions::default()
        }),
        "global" => {
            let mut extensions = with_table(TableOptions::default());
            extensions.push(style().into());
            extensions
        }
        "both" => {
            let mut extensions = with_table(TableOptions {
                html_attributes: object!({"class": "b", "style": "color: red; width: 9px"}),
                ..TableOptions::default()
            });
            extensions.push(style().into());
            extensions
        }
        "kit" => {
            let mut extensions = inline();
            extensions.extend(table_kit(TableKitOptions {
                table: Some(TableOptions {
                    cell_min_width: 30.0,
                    ..TableOptions::default()
                }),
                ..TableKitOptions::default()
            }));
            extensions
        }
        "partial" => vec![
            document().into(),
            paragraph(Default::default()).into(),
            text().into(),
            table_row(TableRowOptions::default()).into(),
            table_cell(TableCellOptions::default()).into(),
            table_header(TableHeaderOptions::default()).into(),
        ],
        other => panic!("no configuration {other}"),
    }
    .into()
}

/// The `width` of each `col` that each table's `querySelectorAll("colgroup > col")` finds, which
/// TableCell's colwidth reads.
fn queries(inputs: &Map) -> Value {
    let html = inputs["html"].as_str().expect("HTML");
    let dom = HtmlDom::parse_document(&format!("<!DOCTYPE html><html><body>{html}</body></html>"));
    let body = dom.body().expect("a body");
    let tables: Vec<Value> = body
        .query_selector_all("table")
        .expect("a selector")
        .iter()
        .map(|table| {
            let cols = table
                .query_selector_all("colgroup > col")
                .expect("a selector");
            let width = |col: &tarnish_html::HtmlNode| {
                col.attribute("width").map_or(Value::Null, Value::String)
            };
            Value::Array(cols.iter().map(width).collect())
        })
        .collect();
    json!({ "tables": tables })
}

fn replay() -> Replay {
    Replay::new("tiptap-tables", configuration).with("queries", &["html"], queries)
}

#[test]
fn converts_as_tiptap_does() {
    replay().check();
}

#[test]
fn converts_tables_nested_as_deeply_as_memory_allows() {
    replay().check_nested(&NESTED);
}

const DOCUMENTS: usize = 20_000;
const PARSES: usize = 2_000;
/// Each level's Markdown holds the next's twice, once as its cell and once as the dashes under
/// its header, which are as wide, so the Markdown doubles with each level.
const MARKDOWNS: usize = 16;

/// A table `depth` deep in a cell of a table, around a paragraph's text, as JSON.
fn nested_table(depth: usize) -> Value {
    let paragraph = json!({"type": "paragraph", "content": [{"type": "text", "text": "x"}]});
    let nested = (0..depth).fold(paragraph, |inner, _| {
        let cell = json!({"type": "tableCell", "content": [inner]});
        json!({"type": "table", "content": [{"type": "tableRow", "content": [cell]}]})
    });
    json!({"type": "doc", "content": [nested]})
}

/// The table a table cell's HTML parses to, with the cell's default attributes.
fn parsed_table(depth: usize) -> Value {
    let paragraph = json!({"type": "paragraph", "content": [{"type": "text", "text": "x"}]});
    let attrs = json!({"colspan": 1, "rowspan": 1, "colwidth": null, "align": null});
    let nested = (0..depth).fold(paragraph, |inner, _| {
        let cell = json!({"type": "tableCell", "attrs": attrs.clone(), "content": [inner]});
        json!({"type": "table", "content": [{"type": "tableRow", "content": [cell]}]})
    });
    json!({"doc": {"type": "doc", "content": [nested]}})
}

/// The Markdown of a table of one cell, which has no header, holding `cell`'s text.
fn table_markdown(cell: &str) -> String {
    let width = cell.len().max(3);
    let pad = " ".repeat(width - cell.len());
    let (spaces, dashes) = (" ".repeat(width), "-".repeat(width));
    format!("\n| {spaces} |\n| {dashes} |\n| {cell}{pad} |\n")
}

const NESTED: [Nested; 3] = [
    Nested {
        what: "HTML of tables in cells",
        configuration: "tables",
        section: "serializesHTML",
        depth: DOCUMENTS,
        input: nested_table,
        output: |depth| {
            let open = concat!(
                r#"<table style="min-width: 25px;"><colgroup><col style="min-width: 25px;">"#,
                r#"</colgroup><tbody><tr><td colspan="1" rowspan="1">"#,
            );
            let close = "</td></tr></tbody></table>";
            json!({"html": open.repeat(depth) + "<p>x</p>" + &close.repeat(depth)})
        },
    },
    // A cell's Markdown is its table's on one line: each line break a `<br>`, and each run of
    // spaces one.
    Nested {
        what: "Markdown of tables in cells",
        configuration: "tables",
        section: "serializesMarkdown",
        depth: MARKDOWNS,
        input: nested_table,
        output: |depth| {
            let mut cell = "x".to_owned();
            for _ in 1..depth {
                let dashes = "-".repeat(cell.len().max(3));
                cell = format!("<br>| |<br>| {dashes} |<br>| {cell} |<br>");
            }
            json!({"markdown": table_markdown(&cell)})
        },
    },
    Nested {
        what: "<table><tr><td>",
        configuration: "tables",
        section: "parsesHTML",
        depth: PARSES,
        input: |depth| {
            let html = "<table><tr><td>".repeat(depth) + "x" + &"</td></tr></table>".repeat(depth);
            Value::String(html)
        },
        output: parsed_table,
    },
];
