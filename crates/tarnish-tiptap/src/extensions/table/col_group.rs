//! `createColGroup` and `getColStyleDeclaration`: the `<colgroup>` a table renders from the
//! widths of its first row's cells, and the width the table takes from them. The cells'
//! attributes can be any JSON, which these compare, index and add as JavaScript does.

use tarnish::Node;
use tarnish_js::json::Value;
use tarnish_js::{self as js, Result};

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
    let mut total_width = Sum::Number(0.0);
    let mut fixed_width = true;
    let mut cols = Vec::new();
    for i in 0..row.child_count() {
        let cell = row.child(i)?;
        let attrs = cell.attrs_view();
        let colspan = attrs.get("colspan").map(|colspan| colspan.to_value());
        let colwidth = attrs.get("colwidth").map(|colwidth| colwidth.to_value());
        let mut j = 0;
        while f64::from(j) < js::to_number(colspan.as_ref())? {
            // `colwidth && colwidth[j]`, as far as its truthiness: a falsy width is no width.
            let has_width = colwidth
                .as_ref()
                .filter(|colwidth| js::truthy(Some(colwidth)))
                .and_then(|colwidth| index(colwidth, j))
                .filter(|width| js::truthy(Some(width)));
            total_width = total_width.add(match &has_width {
                Some(width) => primitive(width)?,
                None => Primitive::Number(cell_min_width),
            });
            if has_width.is_none() {
                fixed_width = false;
            }
            let (property, value) = get_col_style_declaration(cell_min_width, has_width.as_ref())?;
            let style = AttrValue::from(format!("{property}: {value}"));
            cols.push(DomSpec::element(
                "col",
                [("style", style)].into(),
                Vec::new(),
            ));
            j += 1;
        }
    }
    let total_width = total_width.to_js_string() + "px";
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
            let width = math_max(js::to_number(Some(width))?, min_width);
            ("width", js::number_to_string(width) + "px")
        }
        None => ("min-width", js::number_to_string(min_width) + "px"),
    })
}

/// `value[j]`: an array's item, a string's UTF-16 unit, an object's property `"j"`.
fn index(value: &Value, j: u32) -> Option<Value> {
    let at = usize::try_from(j).ok()?;
    match value {
        Value::Array(items) => items.get(at).cloned(),
        Value::String(string) => {
            let unit = string.encode_utf16().nth(at)?;
            Some(Value::String(String::from_utf16_lossy(&[unit])))
        }
        Value::Object(map) => map.get(j.to_string().as_str()).cloned(),
        Value::Null | Value::Bool(_) | Value::Number(_) => None,
    }
}

/// `Math.max(a, b)`: `NaN` if either is, and `+0` over `-0`.
fn math_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_positive() { a } else { b }
    } else {
        a.max(b)
    }
}

/// What `+` reads of a value: `ToPrimitive`, which joins an array and names an object.
enum Primitive {
    Number(f64),
    String(String),
    Bool(bool),
}

fn primitive(value: &Value) -> Result<Primitive> {
    Ok(match value {
        Value::Null => Primitive::Number(0.0),
        Value::Bool(boolean) => Primitive::Bool(*boolean),
        Value::Number(number) => Primitive::Number(number.as_f64()),
        Value::String(string) => Primitive::String(string.clone()),
        Value::Array(_) | Value::Object(_) => Primitive::String(js::to_string(value)?),
    })
}

/// A total `+=` makes: a number, until a string joins it.
enum Sum {
    Number(f64),
    String(String),
}

impl Sum {
    fn add(self, value: Primitive) -> Sum {
        match (self, value) {
            (Sum::String(sum), Primitive::String(value)) => Sum::String(sum + &value),
            (Sum::String(sum), Primitive::Number(value)) => {
                Sum::String(sum + &js::number_to_string(value))
            }
            (Sum::String(sum), Primitive::Bool(value)) => Sum::String(format!("{sum}{value}")),
            (Sum::Number(sum), Primitive::String(value)) => {
                Sum::String(js::number_to_string(sum) + &value)
            }
            (Sum::Number(sum), Primitive::Number(value)) => Sum::Number(sum + value),
            (Sum::Number(sum), Primitive::Bool(value)) => {
                Sum::Number(sum + f64::from(u8::from(value)))
            }
        }
    }

    fn to_js_string(&self) -> String {
        match self {
            Sum::Number(sum) => js::number_to_string(*sum),
            Sum::String(sum) => sum.clone(),
        }
    }
}
