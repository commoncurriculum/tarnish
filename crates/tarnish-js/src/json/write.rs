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
    while let Some(index) = rest
        .bytes()
        .position(|byte| byte < 0x20 || byte == b'"' || byte == b'\\')
    {
        out.push_str(&rest[..index]);
        write_escape(out, rest.as_bytes()[index].into());
        rest = &rest[index + 1..];
    }
    out.push_str(rest);
    out.push('"');
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
