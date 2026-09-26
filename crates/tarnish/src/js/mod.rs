//! JavaScript's semantics for the values ProseMirror handles: its numbers, truthiness, `typeof`
//! and `String()`, and `JSON` in the module.

pub mod json;

use std::borrow::Cow;

use crate::json::{Map, Number, Value};

/// `Number.MAX_SAFE_INTEGER`.
pub const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// A JavaScript number as JSON holds it: an integer where JavaScript holds one exactly, and
/// `null` where it isn't finite, which is what [`json::stringify`] writes.
pub fn number(double: f64) -> Value {
    if double.fract() == 0.0 && double.abs() <= MAX_SAFE_INTEGER {
        Value::Number(Number::from(double as i64))
    } else {
        Number::from_f64(double).map_or(Value::Null, Value::Number)
    }
}

/// `String(double)`.
pub fn number_to_string(double: f64) -> String {
    ryu_js::Buffer::new().format(double).to_string()
}

/// JavaScript truthiness, `None` being `undefined`.
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::String(string)) => !string.is_empty(),
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Some(_) => true,
    }
}

/// `typeof value`, with `null` as `"null"`, as an attribute's
/// [`validate`](crate::AttributeSpec) type list names it. `None` is `undefined`.
pub fn type_of(value: Option<&Value>) -> &'static str {
    match value {
        None => "undefined",
        Some(Value::Null) => "null",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Number(_)) => "number",
        Some(Value::String(_)) => "string",
        Some(Value::Array(_) | Value::Object(_)) => "object",
    }
}

/// `a === b` for numbers, which JavaScript holds as doubles, however JSON wrote them.
pub fn same_number(a: &Number, b: &Number) -> bool {
    a.as_f64() == b.as_f64()
}

/// `String(value)`: an array joins its items with commas, `null` among them as nothing.
pub fn to_string(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Number(number) => number_to_string(number.as_f64().unwrap_or(f64::NAN)),
        Value::String(string) => string.clone(),
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => to_string(other),
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// `String(value)`, `None` being `undefined`, borrowing a string rather than copying it.
pub fn string(value: Option<&Value>) -> Cow<'_, str> {
    match value {
        Some(Value::String(string)) => Cow::Borrowed(string),
        Some(value) => Cow::Owned(to_string(value)),
        None => Cow::Borrowed("undefined"),
    }
}

/// Attributes as a type's `create` reads them: `null` and `undefined` are none, and a value
/// that isn't an object has no properties.
pub fn attrs(value: Option<&Value>) -> Option<&Map> {
    static EMPTY: Map = Map::new();
    match value {
        None | Some(Value::Null) => None,
        Some(Value::Object(object)) => Some(object),
        Some(_) => Some(&EMPTY),
    }
}
