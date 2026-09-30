//! What JavaScript does as it reads a JSON value: reading a property, calling an array method,
//! iterating, keying a `Map`, and the `TypeError` V8 throws where it can't. `None` is
//! `undefined`.

use std::borrow::Cow;
use std::hash::{Hash, Hasher};

use crate::json::Value;
use crate::{Error, Result, number_to_string};

/// JavaScript's two values without properties.
#[derive(Clone, Copy, Debug)]
pub enum Nullish {
    Null,
    Undefined,
}

impl Nullish {
    fn of(value: Option<&Value>) -> Option<Nullish> {
        match value {
            None => Some(Nullish::Undefined),
            Some(Value::Null) => Some(Nullish::Null),
            Some(_) => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Nullish::Null => "null",
            Nullish::Undefined => "undefined",
        }
    }
}

/// The `TypeError` of reading `property` of `value`.
pub fn cannot_read(value: Nullish, property: &str) -> Error {
    Error::Type(format!(
        "Cannot read properties of {} (reading '{property}')",
        value.name()
    ))
}

/// `String(value)` of a string that may be `null`, `None` being `undefined`, such as an
/// element's attribute an optional chain reads.
pub fn nullable_string<S: AsRef<str>>(value: &Option<Option<S>>) -> &str {
    match value {
        None => Nullish::Undefined.name(),
        Some(None) => Nullish::Null.name(),
        Some(Some(string)) => string.as_ref(),
    }
}

/// A value whose `property` the JavaScript reads, which it has as `null` where it's missing.
pub fn non_null<T>(value: Option<T>, property: &str) -> Result<T> {
    value.ok_or_else(|| cannot_read(Nullish::Null, property))
}

/// A value whose `property` the JavaScript reads, which it has as `undefined` where it's
/// missing.
pub fn defined<T>(value: Option<T>, property: &str) -> Result<T> {
    value.ok_or_else(|| cannot_read(Nullish::Undefined, property))
}

/// `object.key`: the own property, as a JSON value inherits none of the names read here, or a
/// `TypeError` for `null` and `undefined`.
pub fn get<'a>(object: Option<&'a Value>, key: &str) -> Result<Option<&'a Value>> {
    match Nullish::of(object) {
        Some(nullish) => Err(cannot_read(nullish, key)),
        None => Ok(optional(object, key)),
    }
}

/// `object?.key`.
pub fn optional<'a>(object: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    debug_assert!(key != "length" && key.parse::<usize>().is_err());
    match object {
        Some(Value::Object(map)) => map.get(key),
        _ => None,
    }
}

/// The items of `value`, whose array method `callee` the JavaScript calls: a `TypeError` when
/// `value` isn't an array. No other JSON value inherits a method of the names called here.
pub fn array_method<'a>(value: Option<&'a Value>, callee: &str) -> Result<&'a [Value]> {
    debug_assert!(!callee.ends_with(".includes") && !callee.ends_with(".indexOf"));
    match value {
        Some(Value::Array(items)) => Ok(items),
        _ => match Nullish::of(value) {
            Some(nullish) => Err(cannot_read(
                nullish,
                callee.rsplit('.').next().unwrap_or(callee),
            )),
            None => Err(not_a_function(callee)),
        },
    }
}

/// The `TypeError` of calling `callee`, which isn't a function, as V8 prints the expression.
pub fn not_a_function(callee: &str) -> Error {
    Error::Type(format!("{callee} is not a function"))
}

/// `value || []`.
pub fn or_empty_array(value: Option<&Value>) -> Option<&Value> {
    Some(crate::or(value, &crate::EMPTY_ARRAY))
}

/// How the JavaScript iterates a value, which decides how V8 words the `TypeError` for one that
/// isn't iterable.
#[derive(Clone, Copy, Debug)]
pub enum Iterating<'s> {
    /// V8 prints the expression, as the source writes it, for `for (… of expression)` and
    /// `[...expression]` over a variable, a property, or a parenthesized expression.
    Expression(&'s str),
    /// V8 describes the value for a destructuring of anything but a variable, for
    /// `Array.from`, and for `for...of` over a logical expression.
    Value,
}

/// What iterating `value` goes through: an array's items, a string's code points, or V8's
/// `TypeError` for a value that isn't iterable.
pub fn iterate<'a>(value: Option<&'a Value>, how: Iterating) -> Result<Cow<'a, [Value]>> {
    // V8 describes a value by its type, and a primitive by its value too.
    let described = match value {
        Some(Value::Array(items)) => return Ok(Cow::Borrowed(items)),
        Some(Value::String(string)) => {
            return Ok(Cow::Owned(
                string
                    .chars()
                    .map(|character| Value::String(character.into()))
                    .collect(),
            ));
        }
        None => "undefined".into(),
        Some(Value::Null) => "object null".into(),
        Some(Value::Bool(boolean)) => format!("boolean {boolean}"),
        Some(Value::Number(number)) => format!(
            "number {}",
            number_to_string(number.as_f64().unwrap_or(f64::NAN))
        ),
        Some(Value::Object(_)) => "object".into(),
    };
    Err(Error::Type(match how {
        Iterating::Expression(source) => format!("{source} is not iterable"),
        Iterating::Value => {
            format!("{described} is not iterable (cannot read property Symbol(Symbol.iterator))")
        }
    }))
}

/// A value as a `Map` or `Set` key, which JavaScript compares with SameValueZero: primitives
/// by value, objects and arrays by identity.
#[derive(Clone, Copy, Debug)]
pub enum SameValueKey<'a> {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(&'a str),
    Reference(&'a Value),
}

impl<'a> SameValueKey<'a> {
    pub fn of(value: Option<&'a Value>) -> SameValueKey<'a> {
        match value {
            None => SameValueKey::Undefined,
            Some(Value::Null) => SameValueKey::Null,
            Some(Value::Bool(boolean)) => SameValueKey::Bool(*boolean),
            Some(Value::Number(number)) => {
                SameValueKey::Number(number.as_f64().unwrap_or(f64::NAN))
            }
            Some(Value::String(string)) => SameValueKey::String(string),
            Some(value) => SameValueKey::Reference(value),
        }
    }

    pub fn as_str(self) -> Option<&'a str> {
        match self {
            SameValueKey::String(string) => Some(string),
            _ => None,
        }
    }

    /// `String(key)`.
    pub fn to_js_string(self) -> Result<Cow<'a, str>> {
        Ok(match self {
            SameValueKey::Undefined => Cow::Borrowed("undefined"),
            SameValueKey::Null => Cow::Borrowed("null"),
            SameValueKey::Bool(boolean) => Cow::Borrowed(if boolean { "true" } else { "false" }),
            SameValueKey::Number(number) => Cow::Owned(number_to_string(number)),
            SameValueKey::String(string) => Cow::Borrowed(string),
            SameValueKey::Reference(value) => Cow::Owned(crate::to_string(value)?),
        })
    }
}

impl PartialEq for SameValueKey<'_> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (SameValueKey::Undefined, SameValueKey::Undefined)
            | (SameValueKey::Null, SameValueKey::Null) => true,
            (SameValueKey::Bool(a), SameValueKey::Bool(b)) => a == b,
            (SameValueKey::Number(a), SameValueKey::Number(b)) => {
                a == b || (a.is_nan() && b.is_nan())
            }
            (SameValueKey::String(a), SameValueKey::String(b)) => a == b,
            (SameValueKey::Reference(a), SameValueKey::Reference(b)) => std::ptr::eq(*a, *b),
            _ => false,
        }
    }
}

impl Eq for SameValueKey<'_> {}

impl Hash for SameValueKey<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            SameValueKey::Undefined | SameValueKey::Null => {}
            SameValueKey::Bool(boolean) => boolean.hash(state),
            // SameValueZero: -0 is 0, and every NaN is the same.
            SameValueKey::Number(number) if *number == 0.0 => 0u64.hash(state),
            SameValueKey::Number(number) if number.is_nan() => 1u64.hash(state),
            SameValueKey::Number(number) => number.to_bits().hash(state),
            SameValueKey::String(string) => string.hash(state),
            SameValueKey::Reference(value) => std::ptr::hash(*value, state),
        }
    }
}

/// `a === b`, which for JSON values, never NaN, is SameValueZero.
pub fn strict_equals(a: Option<&Value>, b: Option<&Value>) -> bool {
    SameValueKey::of(a) == SameValueKey::of(b)
}

/// One of the own enumerable properties `Object.keys` lists: a JSON value, or a string's code
/// unit.
#[derive(Clone, Copy)]
enum Property<'a> {
    Value(&'a Value),
    CodeUnit(u16),
}

impl Property<'_> {
    /// `Object.is(a, b)`, for values JSON holds (never NaN).
    fn same_value(self, other: Property) -> bool {
        match (self, other) {
            (Property::Value(Value::Number(a)), Property::Value(Value::Number(b))) => {
                let (a, b) = (a.as_f64(), b.as_f64());
                a.map(f64::to_bits) == b.map(f64::to_bits)
            }
            (Property::Value(a), Property::Value(b)) => strict_equals(Some(a), Some(b)),
            (Property::CodeUnit(a), Property::CodeUnit(b)) => a == b,
            (Property::CodeUnit(unit), Property::Value(Value::String(string)))
            | (Property::Value(Value::String(string)), Property::CodeUnit(unit)) => {
                let mut units = string.encode_utf16();
                units.next() == Some(unit) && units.next().is_none()
            }
            _ => false,
        }
    }
}

/// `Object.keys(value)` with each key's value.
fn own_properties(value: &Value) -> Vec<(Cow<'_, str>, Property<'_>)> {
    match value {
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| (Cow::Borrowed(key.as_str()), Property::Value(value)))
            .collect(),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(index, item)| (Cow::Owned(index.to_string()), Property::Value(item)))
            .collect(),
        Value::String(string) => string
            .encode_utf16()
            .enumerate()
            .map(|(index, unit)| (Cow::Owned(index.to_string()), Property::CodeUnit(unit)))
            .collect(),
        _ => Vec::new(),
    }
}

/// `Object.keys(a).length === Object.keys(b).length && Object.keys(a).every((key) =>
/// Object.prototype.hasOwnProperty.call(b, key) && Object.is(a[key], b[key]))`.
pub fn same_own_properties(a: &Value, b: &Value) -> bool {
    if let (Value::Object(a), Value::Object(b)) = (a, b) {
        return a.len() == b.len()
            && a.iter().all(|(key, value)| {
                b.get(key)
                    .is_some_and(|other| Property::Value(value).same_value(Property::Value(other)))
            });
    }
    let (a, b) = (own_properties(a), own_properties(b));
    a.len() == b.len()
        && a.iter().all(|(key, value)| {
            b.iter()
                .find(|(other_key, _)| other_key == key)
                .is_some_and(|(_, other)| value.same_value(*other))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::{self, json};

    fn message<T: std::fmt::Debug>(result: Result<T>) -> String {
        result.unwrap_err().to_string()
    }

    // Each as Node throws it.
    #[test]
    fn throws_as_v8_does() {
        let node = json!({"content": {}, "marks": 1.5, "n": null, "t": true});
        assert_eq!(
            message(get(None, "color")),
            "TypeError: Cannot read properties of undefined (reading 'color')"
        );
        assert_eq!(
            message(get(node.get("n"), "a")),
            "TypeError: Cannot read properties of null (reading 'a')"
        );
        assert_eq!(
            message(array_method(node.get("content"), "node.content.map")),
            "TypeError: node.content.map is not a function"
        );
        assert_eq!(
            message(array_method(node.get("attrs"), "node.attrs.map")),
            "TypeError: Cannot read properties of undefined (reading 'map')"
        );
        assert_eq!(
            message(iterate(
                node.get("content"),
                Iterating::Expression("node.content")
            )),
            "TypeError: node.content is not iterable"
        );
        let described = |key: &str| message(iterate(node.get(key), Iterating::Value));
        assert_eq!(
            described("content"),
            "TypeError: object is not iterable (cannot read property Symbol(Symbol.iterator))"
        );
        assert_eq!(
            described("marks"),
            "TypeError: number 1.5 is not iterable (cannot read property Symbol(Symbol.iterator))"
        );
        assert_eq!(
            described("n"),
            "TypeError: object null is not iterable (cannot read property Symbol(Symbol.iterator))"
        );
        assert_eq!(
            described("t"),
            "TypeError: boolean true is not iterable (cannot read property Symbol(Symbol.iterator))"
        );
        assert_eq!(
            described("attrs"),
            "TypeError: undefined is not iterable (cannot read property Symbol(Symbol.iterator))"
        );
        let own = json::from_str(r#"[1, {"toString": 1}]"#).unwrap();
        assert_eq!(
            message(crate::to_string(&own)),
            "TypeError: Cannot convert object to primitive value"
        );
        assert_eq!(
            crate::to_string(&json!([1, null, [2, 3], {}])).unwrap(),
            "1,,2,3,[object Object]"
        );
    }

    #[test]
    fn strings_a_nullable_string_as_javascript_does() {
        let values = [None, Some(None), Some(Some("a"))];
        assert_eq!(
            values.each_ref().map(nullable_string),
            ["undefined", "null", "a"]
        );
    }
}
