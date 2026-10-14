//! `src/utils`: a cell's alignment and column widths as HTML gives them, and the content an
//! empty cell is filled with.

use tarnish::{Fragment, Schema};
use tarnish_html::HtmlNode;
use tarnish_js::json::{Map, Value, json};
use tarnish_js::{self as js, Error, Result};

use crate::{ExtensionAttribute, ParseHtml};

/// `TableCellAlign`.
pub const ALIGNS: [&str; 3] = ["left", "right", "center"];

/// `normalizeTableCellAlign(value)`: the value when it is one of the alignments.
pub fn normalize_table_cell_align(value: Option<&str>) -> Option<&'static str> {
    ALIGNS.into_iter().find(|align| value == Some(*align))
}

/// `parseAlign(element)`: the `text-align` of the style, else the `align` attribute.
fn parse_align(element: &HtmlNode) -> Option<&'static str> {
    let style_align = js::trim(&element.style_value("text-align")).to_lowercase();
    let attr_align = js::trim(&element.attribute("align").unwrap_or_default()).to_lowercase();
    let align = if style_align.is_empty() {
        attr_align
    } else {
        style_align
    };
    normalize_table_cell_align(Some(&align))
}

/// `createAlignAttribute()`.
pub fn create_align_attribute() -> ExtensionAttribute {
    ExtensionAttribute::new("align", Value::Null)
        .parse_html(|element| parse_align(element).map(|align| Value::String(align.into())))
        .render_html(|attributes, rendered| match attributes.get("align") {
            Some(align) if align.truthy() => {
                rendered.merge("style", format!("text-align: {}", align.to_js_string()?))
            }
            _ => Ok(()),
        })
}

/// `parseColgroupWidth(element)`: the `width` of the `<col>` of the cell's column in its
/// table's `<colgroup>`s, which `querySelectorAll` finds in any table inside the table too.
fn parse_colgroup_width(element: &HtmlNode) -> Option<Value> {
    let row = element.parent_element()?;
    let table = element.closest("table").expect("a selector")?;
    let cell_index = row.children().iter().position(|child| child == element)?;
    let cols = table
        .query_selector_all("colgroup > col")
        .expect("a selector");
    let width = cols.get(cell_index)?.attribute("width")?;
    (!width.is_empty()).then(|| Value::Array(vec![js::number(js::parse_int_radix(&width, 10))]))
}

/// `parseColwidth(element)`: the widths of the `colwidth` attribute, else of the column's
/// `<col>`.
pub fn parse_colwidth(element: &HtmlNode) -> Option<Value> {
    match element.attribute("colwidth") {
        Some(colwidth) if !colwidth.is_empty() => Some(Value::Array(
            colwidth
                .split(',')
                .map(|width| js::number(js::parse_int_radix(width, 10)))
                .collect(),
        )),
        _ => parse_colgroup_width(element),
    }
}

/// The attributes TableCell and TableHeader share.
pub fn cell_attributes() -> Vec<ExtensionAttribute> {
    vec![
        ExtensionAttribute::new("colspan", json!(1)),
        ExtensionAttribute::new("rowspan", json!(1)),
        ExtensionAttribute::new("colwidth", Value::Null).parse_html(parse_colwidth),
        create_align_attribute(),
    ]
}

/// `isEmptyCellElement(element)`: whether it has no child elements and only ASCII whitespace,
/// so that a non-breaking space isn't empty.
fn is_empty_cell_element(element: &HtmlNode) -> bool {
    element.children().is_empty()
        && element
            .text_content()
            .bytes()
            .all(|byte| matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | b'\x0C'))
}

/// `fillEmptyCellContent(schema.nodes[name])`: the least content the type can have.
fn fill_empty_cell_content(schema: &Schema, name: &str) -> Result<Fragment<'static>> {
    let cell_type = schema.node_type(name).expect("the cell's type");
    match cell_type.create_and_fill(None, Fragment::empty(), &[])? {
        Some(filled) => Ok(filled.content().clone()),
        None => Err(Error::Other(format!(
            "[tiptap error]: \"{name}\" has no default content to backfill."
        ))),
    }
}

/// The rules TableCell and TableHeader parse their `tag` with: an empty cell is filled, and
/// any other falls through to the second.
pub fn cell_parse_html(tag: &'static str, name: &'static str) -> [ParseHtml; 2] {
    [
        ParseHtml::tag(tag)
            .get_attrs(|element| Ok(is_empty_cell_element(element).then(Map::new)))
            .get_content(move |_, schema| fill_empty_cell_content(schema, name))
            .into(),
        ParseHtml::tag(tag).into(),
    ]
}
