//! `JSON.stringify`, as JavaScript writes values.

use crate::json::{Event, Number, Value, events};

/// `JSON.stringify(value)`, however deeply it nests.
pub fn stringify(value: &Value) -> String {
    let mut out = String::new();
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
            Event::Scalar(Value::Number(number)) => write_number(&mut out, number),
            Event::Scalar(Value::String(string)) => write_string(&mut out, string),
            Event::Open(Value::Array(_)) => {
                out.push('[');
                comma = false;
            }
            Event::Open(_) => {
                out.push('{');
                comma = false;
            }
            Event::Key(key) => {
                write_string(&mut out, key);
                out.push(':');
                comma = false;
            }
            Event::Close(Value::Array(_)) => out.push(']'),
            Event::Close(_) | Event::Scalar(_) => out.push('}'),
        }
    }
    out
}

/// How many arrays and objects `value` nests, itself included.
#[cfg(test)]
pub fn depth(value: &Value) -> usize {
    crate::json::nested(value)
        .filter(|(value, _)| matches!(value, Value::Array(_) | Value::Object(_)))
        .map(|(_, depth)| depth + 1)
        .max()
        .unwrap_or(0)
}

// JavaScript holds every number as a double, so a parsed integer beyond 2^53 has already lost
// its low digits by the time it is written back out.
fn write_number(out: &mut String, number: &Number) {
    let double = number.as_f64().unwrap_or(f64::NAN);
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
        match rest.as_bytes()[index] {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            0x08 => out.push_str("\\b"),
            0x0C => out.push_str("\\f"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            byte => out.push_str(&format!("\\u{byte:04x}")),
        }
        rest = &rest[index + 1..];
    }
    out.push_str(rest);
    out.push('"');
}
