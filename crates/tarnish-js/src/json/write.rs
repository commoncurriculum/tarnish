//! `JSON.stringify`, as JavaScript writes values.

use std::fmt::{self, Write};

use super::{Event, Map, Number, Value, events};

/// `JSON.stringify(value)`, however deeply it nests.
pub fn stringify(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

pub fn write_object(out: &mut String, object: &Map) {
    out.push('{');
    for (index, (key, value)) in object.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write_string(out, key);
        out.push(':');
        write_value(out, value);
    }
    out.push('}');
}

/// `JSON.stringify(value)`, written onto `out`.
pub fn write_value(out: &mut String, value: &Value) {
    let mut comma = false;
    for event in events(value) {
        if comma && !matches!(event, Event::Close(_)) {
            out.push(',');
        }
        comma = true;
        match event {
            Event::Scalar(Value::Null) => out.push_str("null"),
            Event::Scalar(Value::Bool(boolean)) => {
                out.push_str(if *boolean { "true" } else { "false" })
            }
            Event::Scalar(Value::Number(number)) => write_number(out, *number),
            Event::Scalar(Value::String(string)) => write_string(out, string),
            Event::Open(Value::Array(_)) => {
                out.push('[');
                comma = false;
            }
            Event::Open(_) => {
                out.push('{');
                comma = false;
            }
            Event::Key(key) => {
                write_string(out, key);
                out.push(':');
                comma = false;
            }
            Event::Close(Value::Array(_)) => out.push(']'),
            Event::Close(_) | Event::Scalar(_) => out.push('}'),
        }
    }
}

/// A value displays as `JSON.stringify` writes it.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&stringify(self))
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&stringify(self))
    }
}

/// `JSON.stringify(number)`, which is `null` for a number that isn't finite.
pub fn write_number(out: &mut String, number: Number) {
    let double = number.as_f64();
    if double.is_finite() {
        out.push_str(ryu_js::Buffer::new().format_finite(double));
    } else {
        out.push_str("null");
    }
}

pub fn write_string(out: &mut String, string: &str) {
    out.reserve(string.len() + 2);
    out.push('"');
    let mut rest = string;
    while let Some(index) = escape_index(rest.as_bytes()) {
        out.push_str(&rest[..index]);
        write_escape(out, rest.as_bytes()[index].into());
        rest = &rest[index + 1..];
    }
    out.push_str(rest);
    out.push('"');
}

/// Where the first byte `JSON.stringify` escapes is: a control character, `"` or `\`. Text is
/// read eight bytes at a time, as serde_json's and V8's writers skip runs that need no escape.
#[inline]
fn escape_index(bytes: &[u8]) -> Option<usize> {
    let (words, tail) = bytes.as_chunks::<8>();
    let at = match words.iter().position(|word| escapes(*word)) {
        Some(word) => word * 8,
        None => {
            // The last few bytes, padded with a letter, which needs no escape.
            let mut word = [b'a'; 8];
            word[..tail.len()].copy_from_slice(tail);
            if !escapes(word) {
                return None;
            }
            words.len() * 8
        }
    };
    let found = bytes[at..]
        .iter()
        .position(|&byte| byte < 0x20 || byte == b'"' || byte == b'\\')?;
    Some(at + found)
}

/// Whether any of eight bytes is one `JSON.stringify` escapes.
#[inline]
fn escapes(bytes: [u8; 8]) -> bool {
    const ONES: u64 = u64::from_ne_bytes([0x01; 8]);
    const HIGHS: u64 = u64::from_ne_bytes([0x80; 8]);
    // Whether any byte of `word` is below `bound`, which is at most 0x80.
    let below = |word: u64, bound: u8| word.wrapping_sub(ONES * u64::from(bound)) & !word & HIGHS;
    let word = u64::from_ne_bytes(bytes);
    below(word, 0x20)
        | below(word ^ (ONES * u64::from(b'"')), 1)
        | below(word ^ (ONES * u64::from(b'\\')), 1)
        != 0
}

/// [`write_string`] of a string held as UTF-16 units, which may hold a lone surrogate.
pub fn write_units(out: &mut String, units: &[u16]) {
    out.reserve(units.len() + 2);
    out.push('"');
    for decoded in char::decode_utf16(units.iter().copied()) {
        match decoded {
            Ok(character) if character < ' ' || character == '"' || character == '\\' => {
                write_escape(out, character.into())
            }
            Ok(character) => out.push(character),
            Err(lone) => write_escape(out, lone.unpaired_surrogate().into()),
        }
    }
    out.push('"');
}

/// A character or a lone surrogate as `JSON.stringify` escapes it.
fn write_escape(out: &mut String, code: u32) {
    match code {
        0x22 => out.push_str("\\\""),
        0x5C => out.push_str("\\\\"),
        0x08 => out.push_str("\\b"),
        0x0C => out.push_str("\\f"),
        0x0A => out.push_str("\\n"),
        0x0D => out.push_str("\\r"),
        0x09 => out.push_str("\\t"),
        _ => write!(out, "\\u{code:04x}").expect("a string takes any write"),
    }
}

/// `JSON.stringify` of an object holding `entries`.
pub fn stringify_entries<'k, 'v>(entries: impl Iterator<Item = (&'k str, &'v Value)>) -> String {
    // Most attribute trailers fit, where growing from one byte would reallocate several times.
    let mut out = String::with_capacity(64);
    out.push('{');
    for (index, (key, value)) in entries.enumerate() {
        if index > 0 {
            out.push(',');
        }
        write_string(&mut out, key);
        out.push(':');
        write_value(&mut out, value);
    }
    out.push('}');
    out
}

/// `JSON.stringify(value, null, 2)`, however deeply it nests.
pub fn stringify_pretty(value: &Value) -> String {
    let mut out = String::new();
    write_pretty(&mut out, value, 0);
    out
}

fn write_pretty(out: &mut String, value: &Value, depth: usize) {
    let indent = |out: &mut String, depth: usize| out.push_str(&"  ".repeat(depth));
    match value {
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Object(entries) if entries.is_empty() => out.push_str("{}"),
        Value::Array(items) => {
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(",\n");
                }
                indent(out, depth + 1);
                crate::stack::grow(|| write_pretty(out, item, depth + 1));
            }
            out.push('\n');
            indent(out, depth);
            out.push(']');
        }
        Value::Object(entries) => {
            out.push_str("{\n");
            for (index, (key, item)) in entries.iter().enumerate() {
                if index > 0 {
                    out.push_str(",\n");
                }
                indent(out, depth + 1);
                write_string(out, key);
                out.push_str(": ");
                crate::stack::grow(|| write_pretty(out, item, depth + 1));
            }
            out.push('\n');
            indent(out, depth);
            out.push('}');
        }
        scalar => write_value(out, scalar),
    }
}

#[cfg(test)]
mod tests {
    use super::write_string;

    /// Each byte `JSON.stringify` escapes, at every place in strings longer and shorter than the
    /// eight bytes read at a time, among ASCII and multi-byte text, is found and escaped, as a
    /// byte-at-a-time scan escapes it.
    #[test]
    fn escapes_as_a_byte_at_a_time_scan_does() {
        let escape = |string: &str| {
            let mut out = String::from("\"");
            for character in string.chars() {
                match character {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\u{8}' => out.push_str("\\b"),
                    '\u{c}' => out.push_str("\\f"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    control if control < ' ' => out.push_str(&format!("\\u{:04x}", control as u32)),
                    other => out.push(other),
                }
            }
            out + "\""
        };
        let specials = (0..0x20u8)
            .map(char::from)
            .chain(['"', '\\', '\u{7f}', '!', '#', '[', ']']);
        for filler in ["a", "é", "\u{80}", "\u{ffff}", "😀"] {
            for len in 0..20 {
                for at in 0..=len {
                    for special in specials.clone() {
                        let mut string = filler.repeat(len);
                        string.insert(string.len() / len.max(1) * at.min(len), special);
                        let mut out = String::new();
                        write_string(&mut out, &string);
                        assert_eq!(out, escape(&string), "{string:?}");
                    }
                }
            }
        }
    }
}
