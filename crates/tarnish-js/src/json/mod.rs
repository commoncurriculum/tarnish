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
mod write;

pub use convert::IntoValue;
pub use de::{SyntaxError, from_str};
pub use deep::{Event, events};
pub use index::JsonIndex;
pub use macros::{json, object};
pub use map::Map;
pub use write::{
    stringify, stringify_entries, stringify_pretty, write_number, write_object, write_string,
    write_units, write_value,
};
/// An object's key, kept in place up to 24 bytes, which every key the schemas name fits in.
pub type Key = compact_str::CompactString;
pub use serde_json::Number;

pub static NULL: Value = Value::Null;

/// An object without properties, to lend where one is needed.
pub static EMPTY: Map = Map::new();

/// A JSON value. It displays, and debugs, as `JSON.stringify` writes it.
#[derive(Default)]
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
    #[inline]
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    #[inline]
    pub fn is_boolean(&self) -> bool {
        matches!(self, Value::Bool(_))
    }

    #[inline]
    pub fn is_number(&self) -> bool {
        matches!(self, Value::Number(_))
    }

    #[inline]
    pub fn is_string(&self) -> bool {
        matches!(self, Value::String(_))
    }

    #[inline]
    pub fn is_array(&self) -> bool {
        matches!(self, Value::Array(_))
    }

    #[inline]
    pub fn is_object(&self) -> bool {
        matches!(self, Value::Object(_))
    }

    #[inline]
    pub fn is_i64(&self) -> bool {
        matches!(self, Value::Number(number) if number.is_i64())
    }

    #[inline]
    pub fn is_u64(&self) -> bool {
        matches!(self, Value::Number(number) if number.is_u64())
    }

    #[inline]
    pub fn is_f64(&self) -> bool {
        matches!(self, Value::Number(number) if number.is_f64())
    }

    #[inline]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(value) => Some(*value),
            _ => None,
        }
    }

    #[inline]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(value) => Some(value),
            _ => None,
        }
    }

    #[inline]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(number) => number.as_f64(),
            _ => None,
        }
    }

    #[inline]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Number(number) => number.as_i64(),
            _ => None,
        }
    }

    #[inline]
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Number(number) => number.as_u64(),
            _ => None,
        }
    }

    #[inline]
    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    #[inline]
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    #[inline]
    pub fn as_object(&self) -> Option<&Map> {
        match self {
            Value::Object(map) => Some(map),
            _ => None,
        }
    }

    #[inline]
    pub fn as_object_mut(&mut self) -> Option<&mut Map> {
        match self {
            Value::Object(map) => Some(map),
            _ => None,
        }
    }

    #[inline]
    pub fn get<I: JsonIndex + ?Sized>(&self, index: &I) -> Option<&Value> {
        index.index_into(self)
    }

    #[inline]
    pub fn get_mut<I: JsonIndex + ?Sized>(&mut self, index: &I) -> Option<&mut Value> {
        index.index_into_mut(self)
    }

    #[inline]
    pub fn take(&mut self) -> Value {
        std::mem::take(self)
    }

    #[inline]
    pub fn into_string(mut self) -> Option<String> {
        match &mut self {
            Value::String(string) => Some(std::mem::take(string)),
            _ => None,
        }
    }

    #[inline]
    pub fn into_array(mut self) -> Option<Vec<Value>> {
        match &mut self {
            Value::Array(items) => Some(std::mem::take(items)),
            _ => None,
        }
    }

    #[inline]
    pub fn into_object(mut self) -> Option<Map> {
        match &mut self {
            Value::Object(map) => Some(std::mem::take(map)),
            _ => None,
        }
    }
}
