//! What @tiptap/extension-table's Table, TableRow, TableCell and TableHeader make with the
//! `MarkdownManager`, `DOMParser` and `DOMSerializer`, recorded by
//! `harness/record-tiptap-tables.mjs` in each configuration a record names, and tables nested in
//! cells far deeper than a small stack could recurse through.

#[allow(dead_code)]
mod own;

use own::{Converter, input_of};
use tarnish::js::stack::on_dirty_scheduler_stack;
use tarnish::json::{Value, json};
use tarnish_fixtures::{expect, read, records, same};
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
        paragraph().into(),
        text().into(),
        hard_break().into(),
        bold().into(),
        italic().into(),
        heading(HeadingOptions::default()).into(),
        code().into(),
    ]
}

/// `HTMLAttributes` of strings.
fn attributes(attributes: &[(&str, &str)]) -> Vec<(String, String)> {
    let attribute = |(name, value): &(&str, &str)| (name.to_string(), value.to_string());
    attributes.iter().map(attribute).collect()
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

/// The extensions of a configuration the recorder names.
fn configuration(name: &str) -> Vec<Extension> {
    match name {
        // Table's other options change only an editor's plugins and node view.
        "tables" | "resizable" => with_table(TableOptions::default()),
        "attributes" => tables(
            TableOptions {
                html_attributes: attributes(&[
                    ("class", "t"),
                    ("style", "color: red; min-width: 1px"),
                ]),
                ..TableOptions::default()
            },
            TableRowOptions {
                html_attributes: attributes(&[("class", "r")]),
            },
            TableCellOptions {
                html_attributes: attributes(&[("class", "c"), ("style", "padding: 1px")]),
            },
            TableHeaderOptions {
                html_attributes: attributes(&[("class", "h"), ("data-x", "y")]),
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
                html_attributes: attributes(&[("class", "b"), ("style", "color: red; width: 9px")]),
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
            paragraph().into(),
            text().into(),
            table_row(TableRowOptions::default()).into(),
            table_cell(TableCellOptions::default()).into(),
            table_header(TableHeaderOptions::default()).into(),
        ],
        other => panic!("no configuration {other}"),
    }
}

const CONFIGURATIONS: [&str; 10] = [
    "tables",
    "attributes",
    "wrapper",
    "minWidth",
    "zero",
    "resizable",
    "global",
    "both",
    "kit",
    "partial",
];

fn converters() -> Vec<(&'static str, Converter)> {
    CONFIGURATIONS
        .iter()
        .map(|name| (*name, Converter::of(&configuration(name))))
        .collect()
}

fn named(record: &Value) -> &str {
    record["extensions"].as_str().expect("a configuration")
}

/// Holds each record of `section` to what its conversion makes of the record's input, in the
/// configuration the record names.
fn check(section: &str) {
    let fixtures = read("tiptap-tables");
    let converters = converters();
    let mut failures = Vec::new();
    for mut record in records(&fixtures, section).to_vec() {
        let name = named(&record).to_owned();
        let (_, converter) = converters
            .iter()
            .find(|(configuration, _)| *configuration == name)
            .expect("the configuration");
        let fields = record.as_object_mut().expect("a record");
        let input = fields.remove(input_of(section)).expect("an input");
        fields.remove("extensions");
        let actual = converter.convert(section, &input);
        if !same(&record, &actual) {
            failures.push(format!(
                "{name}: {input}\n  Tiptap: {record}\n  tarnish-tiptap: {actual}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

#[test]
fn parses_markdown_as_tiptap_does() {
    check("parsesMarkdown");
}

#[test]
fn serializes_markdown_as_tiptap_does() {
    check("serializesMarkdown");
}

#[test]
fn parses_html_as_tiptap_does() {
    check("parsesHTML");
}

#[test]
fn serializes_html_as_tiptap_does() {
    check("serializesHTML");
}

/// Each configuration's schema, with `tableRole` only where Table's `extendNodeSchema` puts it.
#[test]
fn builds_the_schemas_tiptap_does() {
    own::check_schemas("tiptap-tables", |name| {
        Converter::of(&configuration(name.expect("a configuration")))
    });
}

const DOCUMENTS: usize = 20_000;
const PARSES: usize = 2_000;
/// Blockquotes nest tables less deeply, as each level lexes the rest of its source again.
const QUOTES: usize = 500;
/// Each level's Markdown holds the next's twice, once as its cell and once as the dashes under
/// its header, which are as wide, so the Markdown doubles with each level.
const MARKDOWNS: usize = 16;

/// An input nested `depth` deep, and what the conversion a `section` of `tiptap-tables.json`
/// records makes of it, which the test folds from the depths below and holds to Tiptap's at the
/// depths the recorder records.
struct Nested {
    what: &'static str,
    section: &'static str,
    depth: usize,
    input: fn(usize) -> Value,
    output: fn(usize) -> Value,
}

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

/// A table of one header cell holding `content`, as Markdown parses to it.
fn header_table(content: Value) -> Value {
    let header =
        json!({"type": "tableHeader", "content": [{"type": "paragraph", "content": [content]}]});
    let table = json!({"type": "table", "content": [{"type": "tableRow", "content": [header]}]});
    json!({"doc": {"type": "doc", "content": [table]}})
}

/// The Markdown of a table of one cell, which has no header, holding `cell`'s text.
fn table_markdown(cell: &str) -> String {
    let width = cell.len().max(3);
    let pad = " ".repeat(width - cell.len());
    let (spaces, dashes) = (" ".repeat(width), "-".repeat(width));
    format!("\n| {spaces} |\n| {dashes} |\n| {cell}{pad} |\n")
}

const NESTED: [Nested; 5] = [
    Nested {
        what: "HTML of tables in cells",
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
        section: "parsesHTML",
        depth: PARSES,
        input: |depth| {
            let html = "<table><tr><td>".repeat(depth) + "x" + &"</td></tr></table>".repeat(depth);
            Value::String(html)
        },
        output: parsed_table,
    },
    // Without an extension for blockquotes, the manager parses a blockquote's content.
    Nested {
        what: "tables in blockquotes",
        section: "parsesMarkdown",
        depth: QUOTES,
        input: |depth| {
            Value::String("> ".repeat(depth) + "| a |\n" + &"> ".repeat(depth) + "| - |")
        },
        output: |_| header_table(json!({"type": "text", "text": "a"})),
    },
    // Table's tokenizer lexes the innermost table again, its code span's pipe escaped.
    Nested {
        what: "tables with code spans in blockquotes",
        section: "parsesMarkdown",
        depth: QUOTES,
        input: |depth| {
            let quotes = "> ".repeat(depth);
            Value::String(format!("{quotes}| `a|b` |\n{quotes}| - |"))
        },
        output: |_| {
            header_table(json!({"type": "text", "text": "a|b", "marks": [{"type": "code"}]}))
        },
    },
];

#[test]
fn converts_tables_nested_as_deeply_as_memory_allows() {
    let fixtures = read("tiptap-tables");
    for nested in &NESTED {
        let input = input_of(nested.section);
        for depth in 1..=3 {
            let given = (nested.input)(depth);
            let record = records(&fixtures, nested.section)
                .iter()
                .find(|record| named(record) == "tables" && same(&record[input], &given));
            let record = record.unwrap_or_else(|| panic!("no record of {given}"));
            expect(record, &[input, "extensions"], (nested.output)(depth));
        }
    }
    on_dirty_scheduler_stack(|| {
        let converter = Converter::of(&configuration("tables"));
        for nested in &NESTED {
            let converted = converter.convert(nested.section, &(nested.input)(nested.depth));
            let expected = (nested.output)(nested.depth);
            assert!(same(&expected, &converted), "{}", nested.what);
        }
    });
}

/// `querySelectorAll("colgroup > col")` of each table, which TableCell's colwidth reads.
#[test]
fn finds_the_cols_linkedom_does() {
    let fixtures = read("tiptap-tables");
    for record in records(&fixtures, "queries") {
        let html = record["html"].as_str().expect("HTML");
        let dom =
            HtmlDom::parse_document(&format!("<!DOCTYPE html><html><body>{html}</body></html>"));
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
        expect(record, &["html"], json!({ "tables": tables }));
    }
}
