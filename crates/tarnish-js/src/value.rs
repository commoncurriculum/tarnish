//! What JavaScript does as it reads a JSON value: reading a property, calling an array method,
//! iterating, keying a `Map`. Where the JS throws, these return JavaScriptCore's TypeError,
//! which quotes the expression as the source writes it. `None` is `undefined`.

use std::borrow::Cow;
use std::hash::{Hash, Hasher};

use super::JsError;
use tarnish::json::Value;

fn not_an_object(value: Option<&Value>, source: &str) -> JsError {
    let value = if value.is_none() { "undefined" } else { "null" };
    JsError::type_error(format!("{value} is not an object (evaluating '{source}')"))
}

/// `object.key`, where `source` is how the JS writes it: the own property, as a JSON value
/// inherits none of the names read here, or a TypeError for `null` and `undefined`.
pub fn get<'a>(
    object: Option<&'a Value>,
    key: &str,
    source: &str,
) -> Result<Option<&'a Value>, JsError> {
    match object {
        None | Some(Value::Null) => Err(not_an_object(object, source)),
        Some(value) => Ok(optional(Some(value), key)),
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

/// The items of `value`, whose array method `callee` the JS calls in `call`: a TypeError when
/// `value` isn't an array. No other JSON value inherits a method of the names called here, but
/// an object may hold a key of that name, which the TypeError quotes.
pub fn array_method<'a>(
    value: Option<&'a Value>,
    callee: &str,
    call: &str,
) -> Result<&'a [Value], JsError> {
    debug_assert!(!callee.ends_with(".includes") && !callee.ends_with(".indexOf"));
    match value {
        Some(Value::Array(items)) => Ok(items),
        None | Some(Value::Null) => Err(not_an_object(value, callee)),
        Some(_) => {
            let method = callee.rsplit('.').next().unwrap_or(callee);
            Err(not_a_function(callee, call, optional(value, method)))
        }
    }
}

/// The TypeError for calling `callee` in `call`, where it reads `found`.
pub fn not_a_function(callee: &str, call: &str, found: Option<&Value>) -> JsError {
    JsError::type_error(format!(
        "{callee} is not a function. (In '{call}', '{callee}' is {})",
        describe(found)
    ))
}

/// How JavaScriptCore's TypeErrors quote a value, as `errorDescriptionForValue` writes it.
pub fn describe(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(boolean)) => boolean.to_string(),
        Some(Value::Number(number)) => super::number_to_string(number.as_f64().unwrap_or(f64::NAN)),
        Some(Value::String(string)) => format!("\"{string}\""),
        Some(Value::Array(_)) => "an instance of Array".into(),
        Some(Value::Object(_)) => "an instance of Object".into(),
    }
}

/// `value || []`.
pub fn or_empty_array(value: Option<&Value>) -> Option<&Value> {
    Some(super::or(value, &super::EMPTY_ARRAY))
}

/// What `for (… of value)` and `const [a, ...rest] = value` go through: an array's items, a
/// string's code points, or JavaScriptCore's TypeError for a value that isn't iterable.
///
/// Once JavaScriptCore has optimized a function, the TypeError it throws for destructuring
/// reads `undefined is not a function (near '...')` instead; a fresh worker's is this one.
pub fn iterate<'a>(value: Option<&'a Value>, source: &str) -> Result<Cow<'a, [Value]>, JsError> {
    let refused = match value {
        Some(Value::Array(items)) => return Ok(Cow::Borrowed(items)),
        Some(Value::String(string)) => {
            return Ok(Cow::Owned(
                string
                    .chars()
                    .map(|character| Value::String(character.into()))
                    .collect(),
            ));
        }
        None | Some(Value::Null) => return Err(not_an_object(value, source)),
        Some(Value::Object(_)) => "{}",
        Some(Value::Number(_)) => "number",
        Some(Value::Bool(true)) => "true",
        Some(Value::Bool(false)) => "false",
    };
    Err(JsError::type_error(format!("{refused} is not iterable")))
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
    pub fn to_js_string(self) -> Result<Cow<'a, str>, JsError> {
        Ok(match self {
            SameValueKey::Undefined => Cow::Borrowed("undefined"),
            SameValueKey::Null => Cow::Borrowed("null"),
            SameValueKey::Bool(boolean) => Cow::Borrowed(if boolean { "true" } else { "false" }),
            SameValueKey::Number(number) => Cow::Owned(super::number_to_string(number)),
            SameValueKey::String(string) => Cow::Borrowed(string),
            SameValueKey::Reference(value) => Cow::Owned(super::to_string(value)?),
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
