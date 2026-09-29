//! `JSON`: what JavaScript parses and writes.

pub use tarnish::js::json::{stringify, write_string, write_value};

use tarnish::json::Value;

/// `JSON.parse(text)`, or `None` where it throws.
pub fn parse(text: &str) -> Option<Value> {
    tarnish::json::from_str(text).ok()
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

/// `JSON.stringify(value, null, 2)`, for zod's issues, which nest only as deep as a schema.
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
                write_pretty(out, item, depth + 1);
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
                write_pretty(out, item, depth + 1);
            }
            out.push('\n');
            indent(out, depth);
            out.push('}');
        }
        scalar => write_value(out, scalar),
    }
}
