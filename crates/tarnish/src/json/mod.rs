//! JSON values with serde_json's behavior under `preserve_order`, on a lighter object: a
//! vector of entries in insertion order, searched in place of serde_json's hashed `IndexMap`.
//! The objects documents hold have a handful of keys, so a search is cheaper than hashing, and
//! building and dropping them allocates less.

mod convert;
mod de;
mod deep;
mod index;
mod macros;
mod map;

use std::fmt;

pub use convert::IntoValue;
pub use de::from_str;
pub use deep::{Event, events};
pub use index::JsonIndex;
pub use macros::{json, object};
pub use map::Map;
/// An object's key, kept in place up to 24 bytes, which every key the schemas name fits in.
pub type Key = compact_str::CompactString;
pub use serde_json::Number;

pub static NULL: Value = Value::Null;

/// An object without properties, to lend where one is needed.
pub static EMPTY: Map = Map::new();

#[derive(Debug, Default)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Value>),
    Object(Map),
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn is_boolean(&self) -> bool {
        matches!(self, Value::Bool(_))
    }

    pub fn is_number(&self) -> bool {
        matches!(self, Value::Number(_))
    }

    pub fn is_string(&self) -> bool {
        matches!(self, Value::String(_))
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Value::Array(_))
    }

    pub fn is_object(&self) -> bool {
        matches!(self, Value::Object(_))
    }

    pub fn is_i64(&self) -> bool {
        matches!(self, Value::Number(number) if number.is_i64())
    }

    pub fn is_u64(&self) -> bool {
        matches!(self, Value::Number(number) if number.is_u64())
    }

    pub fn is_f64(&self) -> bool {
        matches!(self, Value::Number(number) if number.is_f64())
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(number) => number.as_f64(),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Number(number) => number.as_i64(),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Number(number) => number.as_u64(),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Map> {
        match self {
            Value::Object(map) => Some(map),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut Map> {
        match self {
            Value::Object(map) => Some(map),
            _ => None,
        }
    }

    pub fn get<I: JsonIndex + ?Sized>(&self, index: &I) -> Option<&Value> {
        index.index_into(self)
    }

    pub fn get_mut<I: JsonIndex + ?Sized>(&mut self, index: &I) -> Option<&mut Value> {
        index.index_into_mut(self)
    }

    pub fn take(&mut self) -> Value {
        std::mem::take(self)
    }

    pub fn into_string(mut self) -> Option<String> {
        match &mut self {
            Value::String(string) => Some(std::mem::take(string)),
            _ => None,
        }
    }

    pub fn into_array(mut self) -> Option<Vec<Value>> {
        match &mut self {
            Value::Array(items) => Some(std::mem::take(items)),
            _ => None,
        }
    }

    pub fn into_object(mut self) -> Option<Map> {
        match &mut self {
            Value::Object(map) => Some(std::mem::take(map)),
            _ => None,
        }
    }
}

impl fmt::Display for Value {
    /// Compact JSON, as serde_json writes it.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn write_string(out: &mut fmt::Formatter<'_>, text: &str) -> fmt::Result {
            out.write_str("\"")?;
            for character in text.chars() {
                match character {
                    '"' => out.write_str("\\\"")?,
                    '\\' => out.write_str("\\\\")?,
                    '\n' => out.write_str("\\n")?,
                    '\r' => out.write_str("\\r")?,
                    '\t' => out.write_str("\\t")?,
                    '\u{08}' => out.write_str("\\b")?,
                    '\u{0C}' => out.write_str("\\f")?,
                    control if (control as u32) < 0x20 => write!(out, "\\u{:04x}", control as u32)?,
                    other => write!(out, "{other}")?,
                }
            }
            out.write_str("\"")
        }
        match self {
            Value::Null => formatter.write_str("null"),
            Value::Bool(value) => write!(formatter, "{value}"),
            Value::Number(number) => write!(formatter, "{number}"),
            Value::String(text) => write_string(formatter, text),
            Value::Array(items) => {
                formatter.write_str("[")?;
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(",")?;
                    }
                    write!(formatter, "{item}")?;
                }
                formatter.write_str("]")
            }
            Value::Object(map) => {
                formatter.write_str("{")?;
                for (index, (key, item)) in map.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(",")?;
                    }
                    write_string(formatter, key)?;
                    write!(formatter, ":{item}")?;
                }
                formatter.write_str("}")
            }
        }
    }
}
