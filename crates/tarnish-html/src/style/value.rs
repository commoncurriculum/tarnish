//! cssstyle's `valueType`, which guesses what kind of value a string is, and the patterns it
//! and the property parsers match.

use super::is_js_space;
use super::known::{NAMED_COLORS, SYSTEM_COLORS};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Type {
    Integer,
    Number,
    Length,
    Percent,
    Url,
    Color,
    String,
    Angle,
    Keyword,
    Calc,
    /// A malformed `rgb()` or `rgba()`, for which `valueType` returns `undefined`.
    Unknown,
}

pub(super) fn value_type(value: &str) -> Type {
    if is_integer(value) {
        return Type::Integer;
    }
    if is_number(value) {
        return Type::Number;
    }
    if value == "0" || has_unit(value, &LENGTH_UNITS) {
        return Type::Length;
    }
    if is_percent(value) {
        return Type::Percent;
    }
    if function_argument(value, "url").is_some() {
        return Type::Url;
    }
    if function_argument(value, "calc").is_some() {
        return Type::Calc;
    }
    if is_quoted(value) {
        return Type::String;
    }
    if has_unit(value, &["deg", "grad", "rad"]) {
        return Type::Angle;
    }
    if hex_digits(value).is_some() {
        return Type::Color;
    }
    if let Some(arguments) = function_argument(value, "rgb") {
        let parts = split_commas(arguments);
        let color = parts.len() == 3
            && (parts.iter().all(|part| is_percent(part))
                || parts.iter().all(|part| is_integer(part)));
        return if color { Type::Color } else { Type::Unknown };
    }
    if let Some(arguments) = function_argument(value, "rgba") {
        let parts = split_commas(arguments);
        let color = parts.len() == 4
            && (parts[..3].iter().all(|part| is_percent(part))
                || parts[..3].iter().all(|part| is_integer(part)))
            && is_number(parts[3]);
        return if color { Type::Color } else { Type::Unknown };
    }
    if is_hsl(value) {
        return Type::Color;
    }
    let lower = value.to_lowercase();
    let lower = lower.as_str();
    if NAMED_COLORS.binary_search(&lower).is_ok() || SYSTEM_COLORS.binary_search(&lower).is_ok() {
        return Type::Color;
    }
    Type::Keyword
}

const LENGTH_UNITS: [&str; 12] = [
    "in", "cm", "em", "mm", "pt", "pc", "px", "ex", "rem", "vh", "vw", "ch",
];

/// Whether the value is a number with one of these units.
fn has_unit(value: &str, units: &[&str]) -> bool {
    let mut numbers = units.iter().filter_map(|unit| value.strip_suffix(unit));
    numbers.any(is_number)
}

fn digits(text: &str) -> usize {
    text.bytes().take_while(u8::is_ascii_digit).count()
}

fn signless(text: &str) -> &str {
    text.strip_prefix(['-', '+']).unwrap_or(text)
}

/// `/^[-+]?[0-9]+$/`.
pub(super) fn is_integer(text: &str) -> bool {
    let text = signless(text);
    !text.is_empty() && digits(text) == text.len()
}

/// `/^[-+]?[0-9]*\.?[0-9]+$/`.
pub(super) fn is_number(text: &str) -> bool {
    let text = signless(text);
    let (whole, fraction) = text.split_once('.').unwrap_or(("", text));
    digits(whole) == whole.len() && !fraction.is_empty() && digits(fraction) == fraction.len()
}

/// `/^[-+]?[0-9]*\.?[0-9]+%$/`.
pub(super) fn is_percent(text: &str) -> bool {
    text.strip_suffix('%').is_some_and(is_number)
}

/// The digits of `/^#([0-9a-fA-F]{3,4}){1,2}$/`, which takes seven of them too.
pub(super) fn hex_digits(value: &str) -> Option<&str> {
    let hex = value.strip_prefix('#')?;
    let digits = matches!(hex.len(), 3 | 4 | 6 | 7 | 8);
    (digits && hex.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(hex)
}

/// `/^name\(([^)]*)\)$/`: what the parentheses hold.
pub(super) fn function_argument<'v>(value: &'v str, name: &str) -> Option<&'v str> {
    let inner = value
        .strip_prefix(name)?
        .strip_prefix('(')?
        .strip_suffix(')')?;
    (!inner.contains(')')).then_some(inner)
}

/// `/^("[^"]*"|'[^']*')$/`.
fn is_quoted(value: &str) -> bool {
    ['"', '\''].into_iter().any(|quote| {
        value.len() >= 2
            && value.starts_with(quote)
            && value.ends_with(quote)
            && !value[1..value.len() - 1].contains(quote)
    })
}

/// `v.split(/\s*,\s*/)`.
pub(super) fn split_commas(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some(comma) = rest.find(',') {
        parts.push(rest[..comma].trim_end_matches(is_js_space));
        rest = rest[comma + 1..].trim_start_matches(is_js_space);
    }
    parts.push(rest);
    parts
}

/// JavaScript's `parseFloat`: the number the longest decimal literal the text starts with, after
/// white space, gives.
pub(super) fn parse_float(text: &str) -> f64 {
    let text = text.trim_start_matches(is_js_space);
    let unsigned = signless(text);
    if unsigned.starts_with("Infinity") {
        return if text.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    let bytes = unsigned.as_bytes();
    let whole = digits(unsigned);
    let mut end = whole;
    if bytes.get(end) == Some(&b'.') {
        end += 1 + digits(&unsigned[end + 1..]);
    }
    if end == 0 || (end == 1 && whole == 0) {
        return f64::NAN;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let exponent = &unsigned[end + 1..];
        let sign = usize::from(exponent.starts_with(['-', '+']));
        let exponent_digits = digits(&exponent[sign..]);
        if exponent_digits > 0 {
            end += 1 + sign + exponent_digits;
        }
    }
    let sign = text.len() - unsigned.len();
    text[..sign + end].parse().unwrap_or(f64::NAN)
}

/// `/^hsla?\(\s*N\s*,\s*N%\s*,\s*N%\s*(,\s*N\s*)?\)/`, with `N` as `(-?\d+|-?\d*.\d+)`, where
/// `.` is any character but a line break: the positions each step can end at, from those the
/// step before can.
fn is_hsl(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("hsl") else {
        return false;
    };
    let text: Vec<char> = rest.chars().collect();
    let start = if text.first() == Some(&'a') {
        vec![0, 1]
    } else {
        vec![0]
    };
    let literal = |ends: Vec<usize>, wanted: char| -> Vec<usize> {
        let ends = ends.into_iter().filter(|&at| text.get(at) == Some(&wanted));
        ends.map(|at| at + 1).collect()
    };
    let spaces = |ends: Vec<usize>| -> Vec<usize> {
        let mut all = Vec::new();
        for at in ends {
            let run = text[at..].iter().take_while(|&&c| is_js_space(c)).count();
            all.extend(at..=at + run);
        }
        dedup(all)
    };
    let number = |ends: Vec<usize>| -> Vec<usize> {
        let mut all = Vec::new();
        for at in ends {
            let at = if text.get(at) == Some(&'-') {
                at + 1
            } else {
                at
            };
            let whole = text[at..].iter().take_while(|c| c.is_ascii_digit()).count();
            all.extend((1..=whole).map(|length| at + length));
            for before in 0..=whole {
                let dot = at + before;
                if text
                    .get(dot)
                    .is_some_and(|&c| !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
                {
                    let after = text[dot + 1..]
                        .iter()
                        .take_while(|c| c.is_ascii_digit())
                        .count();
                    all.extend((1..=after).map(|length| dot + 1 + length));
                }
            }
        }
        dedup(all)
    };
    let mut ends = literal(start, '(');
    ends = number(spaces(ends));
    ends = literal(spaces(ends), ',');
    ends = literal(number(spaces(ends)), '%');
    ends = literal(spaces(ends), ',');
    ends = literal(number(spaces(ends)), '%');
    ends = spaces(ends);
    let alpha = spaces(number(spaces(literal(ends.clone(), ','))));
    ends.extend(alpha);
    !literal(dedup(ends), ')').is_empty()
}

fn dedup(mut positions: Vec<usize>) -> Vec<usize> {
    positions.sort_unstable();
    positions.dedup();
    positions
}
