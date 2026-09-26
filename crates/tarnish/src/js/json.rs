//! `JSON`: what JavaScript parses and writes.

use crate::json::{Number, Value};

/// `JSON.parse(text)`, or `None` where it throws.
pub fn parse(text: &str) -> Option<Value> {
    crate::json::from_str(text).ok()
}

/// `JSON.stringify(value)`.
pub fn stringify(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => write_number(out, number),
        Value::String(string) => write_string(out, string),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(entries) => {
            out.push('{');
            for (index, (key, item)) in entries.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_string(out, key);
                out.push(':');
                write_value(out, item);
            }
            out.push('}');
        }
    }
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
