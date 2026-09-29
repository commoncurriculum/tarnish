//! JavaScript's string methods, on strings of UTF-16 code units as JavaScript holds them, so
//! that lengths and indices count what JavaScript counts.

pub fn from(string: &str) -> Vec<u16> {
    // Runs of ASCII, most of any text, widen a slice at a time.
    let mut units = Vec::with_capacity(string.len());
    let mut rest = string;
    loop {
        let ascii = rest
            .bytes()
            .position(|byte| !byte.is_ascii())
            .unwrap_or(rest.len());
        units.extend(rest.as_bytes()[..ascii].iter().map(|&byte| u16::from(byte)));
        rest = &rest[ascii..];
        let Some(character) = rest.chars().next() else {
            return units;
        };
        units.extend_from_slice(character.encode_utf16(&mut [0; 2]));
        rest = &rest[character.len_utf8()..];
    }
}

pub fn to_string(units: &[u16]) -> String {
    if units.iter().all(|&unit| unit < 0x80) {
        let ascii = units.iter().map(|&unit| unit as u8).collect();
        return String::from_utf8(ascii).expect("ASCII");
    }
    String::from_utf16_lossy(units)
}

/// Whether `units` is exactly the ASCII `string`.
pub fn is(units: &[u16], string: &str) -> bool {
    units.len() == string.len()
        && units
            .iter()
            .zip(string.bytes())
            .all(|(a, b)| *a == u16::from(b))
}

pub fn starts_with(units: &[u16], string: &str) -> bool {
    units.len() >= string.len() && is(&units[..string.len()], string)
}

/// `indexOf(needle, from)` for an ASCII `needle`.
pub fn index_of_str(units: &[u16], needle: &str, from: usize) -> Option<usize> {
    let needle = needle.as_bytes();
    let Some((&first, rest)) = needle.split_first() else {
        return Some(from.min(units.len()));
    };
    let haystack = units.get(from..)?;
    haystack
        .iter()
        .enumerate()
        .filter(|&(_, &unit)| unit == u16::from(first))
        .map(|(index, _)| index)
        .find(|&index| {
            haystack.len() - index > rest.len()
                && haystack[index + 1..index + 1 + rest.len()]
                    .iter()
                    .zip(rest)
                    .all(|(&unit, &byte)| unit == u16::from(byte))
        })
        .map(|index| index + from)
}

/// `indexOf(unit)` for a single code unit.
pub fn find(units: &[u16], unit: u16) -> Option<usize> {
    units.iter().position(|&each| each == unit)
}

/// `/\s/`: JavaScript's WhiteSpace and LineTerminator, which `trim` strips.
pub fn is_whitespace(unit: u16) -> bool {
    char::from_u32(unit.into()).is_some_and(crate::is_whitespace)
}

pub fn trim(units: &[u16]) -> &[u16] {
    trim_end(trim_start(units))
}

pub fn trim_start(units: &[u16]) -> &[u16] {
    let start = units
        .iter()
        .position(|&unit| !is_whitespace(unit))
        .unwrap_or(units.len());
    &units[start..]
}

pub fn trim_end(units: &[u16]) -> &[u16] {
    let end = units
        .iter()
        .rposition(|&unit| !is_whitespace(unit))
        .map_or(0, |index| index + 1);
    &units[..end]
}

/// `slice(start, end)`, where a negative index counts from the end.
pub fn slice(units: &[u16], start: isize, end: Option<isize>) -> &[u16] {
    let length = units.len() as isize;
    let clamp = |index: isize| {
        if index < 0 {
            (length + index).max(0)
        } else {
            index.min(length)
        }
    };
    let start = clamp(start);
    let end = end.map_or(length, clamp);
    if start >= end {
        &[]
    } else {
        &units[start as usize..end as usize]
    }
}

/// `substring(start)`.
pub fn substring(units: &[u16], start: usize) -> &[u16] {
    &units[start.min(units.len())..]
}

/// `[...string].length`: the code points, a lone surrogate counting as one.
pub fn code_points(units: &[u16]) -> usize {
    char::decode_utf16(units.iter().copied()).count()
}

/// `split("\n", 1)[0]`: up to the first line feed.
pub fn first_line(units: &[u16]) -> &[u16] {
    &units[..find(units, b'\n'.into()).unwrap_or(units.len())]
}

/// `split(separator)` on one code unit.
pub fn split(units: &[u16], separator: u16) -> Vec<&[u16]> {
    units.split(|&unit| unit == separator).collect()
}

pub fn join(parts: &[&[u16]], separator: &[u16]) -> Vec<u16> {
    parts.join(separator)
}

/// `toLowerCase()`.
pub fn to_lower_case(units: &[u16]) -> Vec<u16> {
    from(&to_string(units).to_lowercase())
}

/// `a + b + ...`.
pub fn concat(parts: &[&[u16]]) -> Vec<u16> {
    parts.concat()
}

/// A code unit for an ASCII character.
pub const fn unit(character: u8) -> u16 {
    character as u16
}

// Units the matchers `match` on, which a pattern can only name as constants.
pub const BACKSLASH: u16 = unit(b'\\');
pub const BACKTICK: u16 = unit(b'`');
pub const OPEN_BRACKET: u16 = unit(b'[');
pub const CLOSE_BRACKET: u16 = unit(b']');
pub const OPEN_PAREN: u16 = unit(b'(');
pub const CLOSE_PAREN: u16 = unit(b')');
pub const LESS_THAN: u16 = unit(b'<');

/// A LineTerminator, where a regex's `.` and `$` stop.
pub fn is_line_terminator(unit: u16) -> bool {
    matches!(unit, 0x0A | 0x0D | 0x2028 | 0x2029)
}

#[cfg(test)]
mod tests {
    #[test]
    fn from_encodes_as_encode_utf16() {
        for string in [
            "",
            "plain",
            "é",
            "café au lait",
            "😀",
            "a😀b",
            "日本語のテキスト and ASCII",
            "\u{7F}\u{80}\u{7FF}\u{800}\u{FFFF}\u{10000}",
            "ends in é",
        ] {
            let expected: Vec<u16> = string.encode_utf16().collect();
            assert_eq!(super::from(string), expected, "{string:?}");
        }
    }
}
