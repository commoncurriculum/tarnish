//! JavaScript's values and built-ins as V8 runs them, for a library ported from JavaScript to
//! give what the library gives: JSON values, with JavaScript's conversions and the errors it
//! throws handling them; strings as UTF-16; numbers; `JSON`; and, as features, `RegExp`, on
//! regress, and `localeCompare`, on ICU.
//!
//! [`deadline`] cuts off work that runs past a time limit, [`stack`] lets recursions nest as
//! deeply as the values they walk, and [`random`] gives seeded inputs to tests that hold
//! hand-written code to what it stands in for.

#![forbid(unsafe_code)]

pub mod array;
pub mod deadline;
mod error;
pub mod json;
mod number;
pub mod random;
#[cfg(feature = "regexp")]
pub mod regexp;
pub mod stack;
mod string;
pub mod text;
pub mod units;
pub mod utf16;
pub mod value;

use std::borrow::Cow;

pub use error::{Class, Error, Result};
pub use json::{Key, Map, Value};
pub use number::*;
pub use string::*;
pub use text::Text;

/// JavaScript truthiness, `None` being `undefined`.
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::String(string)) => !string.is_empty(),
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Some(_) => true,
    }
}

/// `typeof value` for what JSON holds, with `null` as `"null"`, as a list of types an attribute
/// may have names it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypeOf {
    Undefined,
    Null,
    Boolean,
    Number,
    String,
    Object,
}

impl TypeOf {
    const ALL: [TypeOf; 6] = [
        TypeOf::Undefined,
        TypeOf::Null,
        TypeOf::Boolean,
        TypeOf::Number,
        TypeOf::String,
        TypeOf::Object,
    ];

    /// `None` being `undefined`.
    pub fn of(value: Option<&Value>) -> TypeOf {
        match value {
            None => TypeOf::Undefined,
            Some(Value::Null) => TypeOf::Null,
            Some(Value::Bool(_)) => TypeOf::Boolean,
            Some(Value::Number(_)) => TypeOf::Number,
            Some(Value::String(_)) => TypeOf::String,
            Some(Value::Array(_) | Value::Object(_)) => TypeOf::Object,
        }
    }

    /// The type a name gives, when a JSON value can have it.
    pub fn named(name: &str) -> Option<TypeOf> {
        TypeOf::ALL
            .into_iter()
            .find(|type_of| type_of.name() == name)
    }

    pub fn name(self) -> &'static str {
        match self {
            TypeOf::Undefined => "undefined",
            TypeOf::Null => "null",
            TypeOf::Boolean => "boolean",
            TypeOf::Number => "number",
            TypeOf::String => "string",
            TypeOf::Object => "object",
        }
    }
}

/// `String(value)`: an array joins its items with commas, `null` among them as nothing.
pub fn to_string(value: &Value) -> Result<String> {
    Ok(match value {
        Value::Null => "null".into(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Number(number) => number_to_string(number.as_f64().unwrap_or(f64::NAN)),
        Value::String(string) => string.clone(),
        Value::Array(items) => {
            let mut parts = Vec::with_capacity(items.len());
            for item in items {
                parts.push(match item {
                    Value::Null => String::new(),
                    other => stack::grow(|| to_string(other))?,
                });
            }
            parts.join(",")
        }
        Value::Object(object) => object_to_string(object)?.into(),
    })
}

/// `String(object)`: `Object.prototype.toString`'s, unless the object has a `toString` of its
/// own, which JSON can't make a function, so that no conversion to a primitive is left.
pub fn object_to_string(object: &Map) -> Result<&'static str> {
    if object.contains_key("toString") {
        return Err(no_primitive());
    }
    Ok("[object Object]")
}

/// What converting an object with no conversion to a primitive throws.
pub fn no_primitive() -> Error {
    Error::Type("Cannot convert object to primitive value".into())
}

/// [`to_string`], borrowing a string.
pub fn string_of(value: &Value) -> Result<Cow<'_, str>> {
    Ok(match value {
        Value::String(string) => Cow::Borrowed(string),
        other => Cow::Owned(to_string(other)?),
    })
}

/// `String(value)`, `None` being `undefined`.
pub fn string(value: Option<&Value>) -> Result<Cow<'_, str>> {
    value.map_or(Ok(Cow::Borrowed("undefined")), string_of)
}

/// `String(value || "")`.
pub fn string_or_empty(value: Option<&Value>) -> Result<String> {
    match value {
        Some(value) if truthy(Some(value)) => to_string(value),
        _ => Ok(String::new()),
    }
}

/// `[]`.
pub static EMPTY_ARRAY: Value = Value::Array(Vec::new());

/// `{}`.
pub static EMPTY_OBJECT: Value = Value::Object(Map::new());

/// `""`.
pub static EMPTY_STRING: Value = Value::String(String::new());

pub static TRUE: Value = Value::Bool(true);

/// `Array.isArray(value) ? value : []`.
pub fn array(value: Option<&Value>) -> &[Value] {
    match value {
        Some(Value::Array(items)) => items,
        _ => &[],
    }
}

/// `value || otherwise`.
pub fn or<'a>(value: Option<&'a Value>, otherwise: &'a Value) -> &'a Value {
    value
        .filter(|value| truthy(Some(value)))
        .unwrap_or(otherwise)
}

/// `value ?? otherwise`.
pub fn coalesce<'a>(value: Option<&'a Value>, otherwise: &'a Value) -> &'a Value {
    value.filter(|value| !value.is_null()).unwrap_or(otherwise)
}

/// An object literal `{ key: value, ... }`, without the entries whose value is `undefined`.
pub fn object_literal<'a>(
    entries: impl IntoIterator<Item = (&'a str, Option<&'a Value>)>,
) -> Value {
    Value::Object(
        entries
            .into_iter()
            .filter_map(|(key, value)| Some((key.into(), value?.clone())))
            .collect(),
    )
}
