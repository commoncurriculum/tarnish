use crate::json::{Number, Value};
use crate::{Result, to_string, trim};

/// `Number.MAX_SAFE_INTEGER`.
pub const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// A JavaScript number as JSON holds it: an integer where JavaScript holds one exactly, and
/// `null` where it isn't finite, which is what `JSON.stringify` writes.
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

/// A number as `JSON.stringify` writes it.
pub enum WrittenNumber {
    /// Digits, which JavaScript writes for an integer below 10^21.
    Integer(i128),
    /// A fraction or an exponent.
    Float(f64),
    /// `null`, for a number that isn't finite.
    Null,
}

pub fn written_number(number: &Number) -> WrittenNumber {
    if let Some(integer) = number
        .as_i64()
        .filter(|integer| integer.unsigned_abs() as f64 <= MAX_SAFE_INTEGER)
    {
        return WrittenNumber::Integer(integer.into());
    }
    let double = number.as_f64().unwrap_or(f64::NAN);
    if !double.is_finite() {
        return WrittenNumber::Null;
    }
    match number_to_string(double).parse() {
        Ok(integer) => WrittenNumber::Integer(integer),
        Err(_) => WrittenNumber::Float(double),
    }
}

/// `a === b` for numbers, which JavaScript holds as doubles, however JSON wrote them.
pub fn same_number(a: &Number, b: &Number) -> bool {
    a.as_f64() == b.as_f64()
}

/// `Number.parseInt(string)`.
pub fn parse_int(string: &str) -> f64 {
    let string = crate::trim_start(string);
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

/// `Number(value)` for a JSON value, `None` being `undefined`.
pub fn to_number(value: Option<&Value>) -> Result<f64> {
    Ok(match value {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(boolean)) => f64::from(u8::from(*boolean)),
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(string)) => string_to_number(string),
        Some(value) => string_to_number(&to_string(value)?),
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
