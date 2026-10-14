//! `roman.ts`: the list markers of numbers, letters and roman numerals.

use tarnish_js::json::Value;
use tarnish_js::{self as js, Error, value};

const ROMAN_NUMERALS: [(f64, &str); 13] = [
    (1000.0, "m"),
    (900.0, "cm"),
    (500.0, "d"),
    (400.0, "cd"),
    (100.0, "c"),
    (90.0, "xc"),
    (50.0, "l"),
    (40.0, "xl"),
    (10.0, "x"),
    (9.0, "ix"),
    (5.0, "v"),
    (4.0, "iv"),
    (1.0, "i"),
];

const ALPHA_NUMERALS: &[u8; 26] = b"abcdefghijklmnopqrstuvwxyz";

/// `toRoman(num)`, in lowercase.
fn to_roman(num: f64) -> Result<String, Error> {
    if num == f64::INFINITY {
        return Err(js::invalid_string_length());
    }
    // The thousands at once: below 2^53, taking 1000 away again and again is exact.
    let mut remaining = num;
    let mut thousands = if remaining >= 1000.0 {
        (remaining / 1000.0).floor()
    } else {
        0.0
    };
    while thousands > 0.0 && remaining - thousands * 1000.0 < 0.0 {
        thousands -= 1.0;
    }
    while remaining - (thousands + 1.0) * 1000.0 >= 0.0 && thousands < js::MAX_SAFE_INTEGER {
        thousands += 1.0;
    }
    let mut result = js::repeat("m", thousands)?;
    remaining -= thousands * 1000.0;
    for (value, numeral) in &ROMAN_NUMERALS[1..] {
        while remaining >= *value {
            result.push_str(numeral);
            remaining -= value;
        }
    }
    Ok(result)
}

/// `fromRoman(roman)`, of a marker of roman numerals' letters: 0 where a letter isn't one.
fn from_roman(roman: &str) -> f64 {
    let lower = roman.to_ascii_lowercase();
    let mut rest = lower.as_str();
    let mut result = 0.0;
    while !rest.is_empty() {
        let Some((value, numeral)) = ROMAN_NUMERALS
            .iter()
            .find(|(_, numeral)| rest.starts_with(numeral))
        else {
            return 0.0;
        };
        result += value;
        rest = &rest[numeral.len()..];
    }
    result
}

fn is_valid_roman(marker: &str) -> bool {
    if marker.is_empty() || !marker.bytes().all(|byte| b"ivxlcdmIVXLCDM".contains(&byte)) {
        return false;
    }
    let value = from_roman(marker);
    if value <= 0.0 {
        return false;
    }
    let expected = to_roman(value).unwrap_or_default();
    match marker == marker.to_ascii_lowercase() {
        true => expected == marker,
        false => expected.to_ascii_uppercase() == marker,
    }
}

/// `fromAlpha(marker)`: a, b, ..., z, aa, ab, ..., zz from 1.
fn from_alpha(marker: &str) -> f64 {
    let lower = marker.to_ascii_lowercase();
    let letter = |byte: u8| f64::from(byte) - f64::from(b'a');
    match lower.as_bytes() {
        [only] => letter(*only) + 1.0,
        [first, second] => (letter(*first) + 1.0) * 26.0 + letter(*second) + 1.0,
        _ => 0.0,
    }
}

/// `ALPHA_NUMERALS[index]`, `None` for `undefined`.
fn alpha_numeral(index: f64) -> Option<char> {
    let valid = index.fract() == 0.0 && (0.0..26.0).contains(&index);
    valid.then(|| char::from(ALPHA_NUMERALS[index as usize]))
}

/// What `toRomanAlpha(num)` returns.
enum Alpha {
    String(String),
    Undefined,
    /// `undefined + undefined`.
    NaN,
}

/// `toRomanAlpha(num)`: the letters of a position, a to zz.
fn to_roman_alpha(num: f64) -> Alpha {
    let numeral = |index| match alpha_numeral(index) {
        Some(letter) => Alpha::String(letter.into()),
        None => Alpha::Undefined,
    };
    if num <= 26.0 {
        return numeral(num - 1.0);
    }
    let first = ((num - 1.0) / 26.0).floor() - 1.0;
    let second = (num - 1.0) % 26.0;
    if first < 0.0 {
        return numeral(second);
    }
    match (alpha_numeral(first), alpha_numeral(second)) {
        (None, None) => Alpha::NaN,
        (first, second) => {
            let part = |letter: Option<char>| letter.map_or("undefined".into(), String::from);
            Alpha::String(part(first) + &part(second))
        }
    }
}

/// `detectMarkerType(marker)`: the `type` of a marker of letters, `None` for numbers.
pub(super) fn detect_marker_type(marker: &str) -> Option<&'static str> {
    if marker.is_empty() || marker.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if is_valid_roman(marker) {
        return Some(match marker == marker.to_ascii_lowercase() {
            true => "i",
            false => "I",
        });
    }
    let letters = |check: fn(&u8) -> bool| {
        (1..=2).contains(&marker.len()) && marker.bytes().all(|byte| check(&byte))
    };
    if letters(u8::is_ascii_lowercase) {
        return Some("a");
    }
    if letters(u8::is_ascii_uppercase) {
        return Some("A");
    }
    None
}

/// `markerToStart(marker)`: the position a marker of letters stands for.
pub(super) fn marker_to_start(marker: &str) -> f64 {
    if !marker.is_empty() && marker.bytes().all(|byte| byte.is_ascii_digit()) {
        return js::parse_int_radix(marker, 10);
    }
    match detect_marker_type(marker) {
        Some("i" | "I") => from_roman(marker),
        Some(_) => match from_alpha(marker) {
            start if start > 0.0 => start,
            _ => 1.0,
        },
        None => match js::parse_int_radix(marker, 10) {
            parsed if parsed.is_nan() => 1.0,
            parsed => parsed,
        },
    }
}

/// `getListMarker(type, index, separator)`: the marker of the item at `index`, in the `type`'s
/// numbering.
pub(crate) fn list_marker(
    kind: Option<&Value>,
    index: f64,
    separator: &str,
) -> Result<String, Error> {
    let position = index + 1.0;
    let number = || js::number_to_string(position) + separator;
    let kind = kind
        .filter(|kind| js::truthy(Some(kind)))
        .and_then(Value::as_str);
    let upper = |alpha: Alpha| match alpha {
        Alpha::String(letters) => Ok(letters.to_uppercase()),
        Alpha::Undefined => Err(value::cannot_read(value::Nullish::Undefined, "toUpperCase")),
        Alpha::NaN => Err(value::not_a_function("toRomanAlpha(...).toUpperCase")),
    };
    Ok(match kind {
        Some("a") => match to_roman_alpha(position) {
            Alpha::String(letters) => letters + separator,
            Alpha::Undefined => ["undefined", separator].concat(),
            Alpha::NaN => ["NaN", separator].concat(),
        },
        Some("A") => upper(to_roman_alpha(position))? + separator,
        Some("i") => to_roman(position)? + separator,
        Some("I") => to_roman(position)?.to_uppercase() + separator,
        _ => number(),
    })
}

#[cfg(test)]
mod tests {
    use super::{from_roman, is_valid_roman, to_roman};

    #[test]
    fn roman_numerals_read_back_as_they_are_written() {
        for number in 1..4000 {
            let roman = to_roman(f64::from(number)).expect("a numeral");
            assert_eq!(from_roman(&roman), f64::from(number));
            assert!(is_valid_roman(&roman) && is_valid_roman(&roman.to_uppercase()));
        }
        assert!(!is_valid_roman("iiii") && !is_valid_roman("ic") && !is_valid_roman("iI"));
    }
}
