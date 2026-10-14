//! `createColGroup` and `getColStyleDeclaration`: the `<colgroup>` a table renders from the
//! widths of its first row's cells, and the width the table takes from them. The cells'
//! attributes can be any JSON, which these compare, index and add as JavaScript does.

use tarnish::Node;
use tarnish_js::json::Value;
use tarnish_js::{self as js, Result, value};

use crate::{AttrValue, DomSpec, SpecAttrs};

/// What `createColGroup` returns for a table with a row.
pub struct ColGroup<'a> {
    pub colgroup: DomSpec<'a>,
    pub table_width: String,
    pub table_min_width: String,
}

/// `createColGroup(node, cellMinWidth)`: `None` for `{}`, which a table without a first child
/// gets.
pub fn create_col_group<'a>(node: &Node, cell_min_width: f64) -> Result<Option<ColGroup<'a>>> {
    let Some(row) = node.first_child() else {
        return Ok(None);
    };
    let min_width = js::number(cell_min_width);
    let mut total_width = js::number(0.0);
    let mut fixed_width = true;
    let mut cols = Vec::new();
    for i in 0..row.child_count() {
        let cell = row.child(i)?;
        let attrs = cell.attrs_view();
        let colspan = attrs.get("colspan").map(|colspan| colspan.to_value());
        let colwidth = attrs.get("colwidth").map(|colwidth| colwidth.to_value());
        let mut j = 0;
        while (j as f64) < js::to_number(colspan.as_ref())? {
            // `colwidth && colwidth[j]`, as far as its truthiness: a falsy width is no width.
            let width = match colwidth
                .as_ref()
                .filter(|colwidth| js::truthy(Some(colwidth)))
            {
                Some(colwidth) => value::index(Some(colwidth), j)?,
                None => None,
            };
            let has_width = width.filter(|width| js::truthy(Some(width)));
            total_width = js::plus(&total_width, has_width.as_deref().unwrap_or(&min_width))?;
            if has_width.is_none() {
                fixed_width = false;
            }
            let (property, value) =
                get_col_style_declaration(cell_min_width, has_width.as_deref())?;
            let style = AttrValue::from(format!("{property}: {value}"));
            cols.push(DomSpec::element(
                "col",
                [("style", style)].into(),
                Vec::new(),
            ));
            j += 1;
        }
    }
    let total_width = js::to_string(&total_width)? + "px";
    let (table_width, table_min_width) = match fixed_width {
        true => (total_width, String::new()),
        false => (String::new(), total_width),
    };
    Ok(Some(ColGroup {
        colgroup: DomSpec::element("colgroup", SpecAttrs::new(), cols),
        table_width,
        table_min_width,
    }))
}

/// `getColStyleDeclaration(minWidth, width)`: the stored width, unless it is below the minimum,
/// or else the minimum as the column's least.
fn get_col_style_declaration(
    min_width: f64,
    width: Option<&Value>,
) -> Result<(&'static str, String)> {
    Ok(match width {
        Some(width) => {
            let width = js::math_max(js::to_number(Some(width))?, min_width);
            ("width", js::number_to_string(width) + "px")
        }
        None => ("min-width", js::number_to_string(min_width) + "px"),
    })
}
