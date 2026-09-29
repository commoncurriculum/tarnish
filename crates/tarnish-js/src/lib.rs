//! JavaScript's built-ins as a library ported from JavaScript uses them, so that the port gives
//! what the library gives: JavaScript's behavior on JSON values here, and its built-in objects in
//! the modules, among them strings as UTF-16 and `RegExp`, on regress.
//!
//! [`deadline`] cuts off work that runs past a time limit, and [`random`] gives seeded inputs to
//! tests that hold hand-written code to the regular expression it stands in for.

#![forbid(unsafe_code)]

pub mod array;
pub mod deadline;
mod error;
pub mod json;
pub mod random;
pub mod regexp;
pub mod units;
pub mod utf16;
pub mod value;

use std::borrow::Cow;

pub use error::{ErrorKind, JsError};
pub use tarnish::js::{MAX_SAFE_INTEGER, number, number_to_string, truthy};
use tarnish::json::Value;
use tarnish::stack;

/// `Number.parseInt(string)`.
pub fn parse_int(string: &str) -> f64 {
    let string = trim_start(string);
    let (sign, string) = match string.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, string.strip_prefix('+').unwrap_or(string)),
    };
    let (radix, string) = match string.get(..2) {
        Some("0x" | "0X") => (16, &string[2..]),
        _ => (10, string),
    };
    let digits: Vec<u32> = string
        .chars()
        .map_while(|digit| digit.to_digit(radix))
        .collect();
    if digits.is_empty() {
        return f64::NAN;
    }
    sign * digits.into_iter().fold(0.0, |number, digit| {
        number * f64::from(radix) + f64::from(digit)
    })
}

/// A string's `length` in JavaScript, which counts UTF-16 code units.
pub fn utf16_len(string: &str) -> usize {
    string.chars().map(char::len_utf16).sum()
}

/// The byte offset of UTF-16 offset `units` in `string`, clamped to the string, and rounded up
/// past a character that the offset would split.
pub fn byte_offset(string: &str, units: usize) -> usize {
    // Through ASCII, a unit is a byte.
    let prefix = &string.as_bytes()[..units.min(string.len())];
    if prefix.is_ascii() {
        return prefix.len();
    }
    let mut seen = 0;
    for (offset, character) in string.char_indices() {
        if seen >= units {
            return offset;
        }
        seen += character.len_utf16();
    }
    string.len()
}

/// Whether JavaScript's `String.prototype.trim` strips `character` (WhiteSpace and
/// LineTerminator), which differs from [`char::is_whitespace`].
pub fn is_whitespace(character: char) -> bool {
    matches!(
        character,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

pub fn trim(string: &str) -> &str {
    trim_end(trim_start(string))
}

/// Whether an ASCII byte is JavaScript whitespace or a line terminator.
fn is_js_ascii_whitespace(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | 0x0B | 0x0C | b'\r' | b' ')
}

pub fn trim_start(string: &str) -> &str {
    let bytes = string.as_bytes();
    let start = bytes
        .iter()
        .position(|&byte| !is_js_ascii_whitespace(byte))
        .unwrap_or(bytes.len());
    // Past ASCII, whitespace takes decoding.
    match bytes.get(start) {
        Some(&byte) if !byte.is_ascii() => string[start..].trim_start_matches(is_whitespace),
        _ => &string[start..],
    }
}

pub fn trim_end(string: &str) -> &str {
    let bytes = string.as_bytes();
    let end = bytes
        .iter()
        .rposition(|&byte| !is_js_ascii_whitespace(byte))
        .map_or(0, |last| last + 1);
    match end.checked_sub(1).map(|last| bytes[last]) {
        Some(byte) if !byte.is_ascii() => string[..end].trim_end_matches(is_whitespace),
        _ => &string[..end],
    }
}

/// `[]`.
pub static EMPTY_ARRAY: Value = Value::Array(Vec::new());

/// `{}`.
pub static EMPTY_OBJECT: Value = Value::Object(tarnish::json::Map::new());

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

/// `String(value || "")`.
pub fn string_or_empty(value: Option<&Value>) -> Result<String, JsError> {
    match value {
        Some(value) if truthy(Some(value)) => to_string(value),
        _ => Ok(String::new()),
    }
}

/// `value || otherwise`.
pub fn or<'a>(value: Option<&'a Value>, otherwise: &'a Value) -> &'a Value {
    value
        .filter(|value| truthy(Some(value)))
        .unwrap_or(otherwise)
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

/// `value ?? otherwise`.
pub fn coalesce<'a>(value: Option<&'a Value>, otherwise: &'a Value) -> &'a Value {
    value.filter(|value| !value.is_null()).unwrap_or(otherwise)
}

/// `String(value)` for a JSON value.
pub fn to_string(value: &Value) -> Result<String, JsError> {
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
        Value::Object(object) => object_to_primitive(object)?.into(),
    })
}

/// [`to_string`], borrowing a string.
pub fn string_of(value: &Value) -> Result<Cow<'_, str>, JsError> {
    Ok(match value {
        Value::String(string) => Cow::Borrowed(string),
        other => Cow::Owned(to_string(other)?),
    })
}

/// `string.repeat(count)`. A string longer than the system will allocate throws as a failed
/// allocation does in JavaScript.
pub fn repeat(string: &str, count: f64) -> Result<String, JsError> {
    let count = if count.is_nan() { 0.0 } else { count.trunc() };
    if count < 0.0 || count.is_infinite() {
        return Err(JsError::range_error(
            "String.prototype.repeat argument must be greater than or equal to 0 and not be \
             Infinity",
        ));
    }
    let mut repeated = String::new();
    let length = (count as usize).checked_mul(string.len());
    deadline::build(length.unwrap_or(usize::MAX))?;
    let Some(length) = length.filter(|&length| repeated.try_reserve_exact(length).is_ok()) else {
        return Err(out_of_memory());
    };
    if length > 0 {
        repeated.push_str(string);
    }
    while repeated.len() < length {
        repeated.extend_from_within(..repeated.len().min(length - repeated.len()));
    }
    Ok(repeated)
}

/// What JavaScript throws when it can't allocate a string.
pub fn out_of_memory() -> JsError {
    JsError::range_error("Out of memory")
}

/// `ToPrimitive` of a JSON object: `valueOf` gives back the object, so `toString` answers,
/// Object.prototype's unless the object has its own, which JSON can't make a function.
fn object_to_primitive(object: &tarnish::json::Map) -> Result<&'static str, JsError> {
    if object.contains_key("toString") {
        return Err(JsError::type_error("No default value"));
    }
    Ok("[object Object]")
}

/// `Number(value)` for a JSON value, or `None` for `undefined`.
pub fn to_number(value: Option<&Value>) -> Result<f64, JsError> {
    Ok(match value {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(boolean)) => f64::from(u8::from(*boolean)),
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(string)) => string_to_number(string),
        Some(array @ Value::Array(_)) => string_to_number(&to_string(array)?),
        Some(Value::Object(object)) => string_to_number(object_to_primitive(object)?),
    })
}

/// `Number(string)`.
pub fn string_to_number(string: &str) -> f64 {
    let trimmed = trim(string);
    if trimmed.is_empty() {
        return 0.0;
    }
    let radix = |prefix: &str, radix: u32| {
        trimmed
            .strip_prefix(prefix)
            .or_else(|| trimmed.strip_prefix(&prefix.to_uppercase()))
            .map(|digits| {
                u128::from_str_radix(digits, radix)
                    .map(|number| number as f64)
                    .unwrap_or(f64::NAN)
            })
    };
    if let Some(number) = radix("0x", 16)
        .or_else(|| radix("0o", 8))
        .or_else(|| radix("0b", 2))
    {
        return number;
    }
    match trimmed {
        "Infinity" | "+Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        _ if trimmed.chars().all(|character| {
            character.is_ascii_digit() || matches!(character, '.' | 'e' | 'E' | '+' | '-')
        }) =>
        {
            trimmed.parse().unwrap_or(f64::NAN)
        }
        _ => f64::NAN,
    }
}

/// `a.localeCompare(b)`, which collates as ICU's root locale does, as Bun's en-US does.
pub fn locale_compare(a: &str, b: &str) -> f64 {
    static COLLATOR: std::sync::LazyLock<icu_collator::CollatorBorrowed<'static>> =
        std::sync::LazyLock::new(|| {
            icu_collator::Collator::try_new(Default::default(), Default::default())
                .expect("the collation data is compiled in")
        });
    match COLLATOR.compare(a, b) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}
