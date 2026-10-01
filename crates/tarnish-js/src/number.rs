use std::fmt;

use crate::json::Value;
use crate::{Result, to_string, trim};

/// `Number.MAX_SAFE_INTEGER`.
pub const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// A JavaScript number: a double, which is what `JSON.parse` reads every number as, so one past
/// a double's range is ±Infinity. Two are equal as `===` compares them: 0 equals -0, and NaN
/// equals nothing, itself included.
#[derive(Clone, Copy, PartialEq)]
pub struct Number(f64);

impl Number {
    #[inline]
    pub fn as_f64(self) -> f64 {
        self.0
    }

    /// The number as an `i64`, when it is an integer an `i64` holds.
    #[inline]
    pub fn as_i64(self) -> Option<i64> {
        // From -2^63, the least `i64`, to 2^63, one past the greatest.
        const RANGE: std::ops::Range<f64> = -9.223_372_036_854_776e18..9.223_372_036_854_776e18;
        (self.0.fract() == 0.0 && RANGE.contains(&self.0)).then_some(self.0 as i64)
    }

    /// The number as a `u64`, when it is an integer a `u64` holds.
    #[inline]
    pub fn as_u64(self) -> Option<u64> {
        // Up to 2^64, one past the greatest `u64`.
        const RANGE: std::ops::Range<f64> = 0.0..1.844_674_407_370_955_2e19;
        (self.0.fract() == 0.0 && RANGE.contains(&self.0)).then_some(self.0 as u64)
    }
}

impl From<f64> for Number {
    #[inline]
    fn from(double: f64) -> Number {
        Number(double)
    }
}

/// An integer as JavaScript holds it: the double nearest it, as `JSON.parse` reads its digits.
macro_rules! number_from_integer {
    ($($integer:ty),*) => {
        $(
            impl From<$integer> for Number {
                #[inline]
                fn from(integer: $integer) -> Number {
                    Number(integer as f64)
                }
            }
        )*
    };
}

number_from_integer!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

/// A number debugs as `String(number)`.
impl fmt::Debug for Number {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(ryu_js::Buffer::new().format(self.0))
    }
}

/// A JavaScript number as a JSON value, which `JSON.stringify` writes as `null` when it isn't
/// finite.
pub fn number(double: f64) -> Value {
    Value::Number(Number(double))
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

pub fn written_number(number: Number) -> WrittenNumber {
    let double = number.as_f64();
    // Every integer up to 2^53 is its own shortest digits, which JavaScript writes.
    if double.fract() == 0.0 && double.abs() <= MAX_SAFE_INTEGER {
        return WrittenNumber::Integer(double as i128);
    }
    if !double.is_finite() {
        return WrittenNumber::Null;
    }
    match number_to_string(double).parse() {
        Ok(integer) => WrittenNumber::Integer(integer),
        Err(_) => WrittenNumber::Float(double),
    }
}

/// `Number.parseInt(string)`.
pub fn parse_int(string: &str) -> f64 {
    parse_int_radix(string, 0)
}

/// `Number.parseInt(string, radix)`, of a radix from 2 to 36, or 0 for none, with which a `0x`
/// prefix reads as hexadecimal and the rest as decimal.
pub fn parse_int_radix(string: &str, radix: u32) -> f64 {
    assert!(radix == 0 || (2..=36).contains(&radix), "radix {radix}");
    let string = crate::trim_start(string);
    let (sign, string) = match string.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, string.strip_prefix('+').unwrap_or(string)),
    };
    let (radix, string) = match (radix, string.get(..2)) {
        (0 | 16, Some("0x" | "0X")) => (16, &string[2..]),
        (0, _) => (10, string),
        _ => (radix, string),
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
        Some(Value::Number(number)) => number.as_f64(),
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

#[cfg(test)]
mod tests {
    use super::{parse_int, parse_int_radix};

    #[test]
    fn parses_an_integer_as_parse_int_does() {
        assert_eq!(parse_int("  -12px"), -12.0);
        assert_eq!(parse_int("0x1A"), 26.0);
        assert_eq!(parse_int_radix("0x1A", 16), 26.0);
        assert_eq!(parse_int_radix("0x1A", 10), 0.0);
        assert_eq!(parse_int_radix("1A", 16), 26.0);
        assert_eq!(parse_int_radix("+7", 10), 7.0);
        assert!(parse_int_radix("x", 10).is_nan());
    }
}
