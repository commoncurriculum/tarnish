//! JSON values as a chunk holds them: attributes, read in place.

use std::fmt;

use super::{Chunk, corrupt};
use crate::js::json::{write_number, write_string};
use crate::json::{Map, Number, Value};
use crate::{js, stack};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Tag {
    Null,
    False,
    True,
    Int,
    UInt,
    Float,
    String,
    Array,
    Object,
}

impl Tag {
    pub fn from_word(word: u32) -> Tag {
        match word {
            0 => Tag::Null,
            1 => Tag::False,
            2 => Tag::True,
            3 => Tag::Int,
            4 => Tag::UInt,
            5 => Tag::Float,
            6 => Tag::String,
            7 => Tag::Array,
            8 => Tag::Object,
            _ => corrupt(),
        }
    }
}

/// A JSON value in a chunk.
#[derive(Clone, Copy)]
pub struct ValueRef<'c> {
    pub(crate) chunk: &'c Chunk<'c>,
    pub(crate) index: u32,
}

/// What a value is, with its contents where they're simple, and an array's or object's length.
#[derive(Clone, Debug)]
pub enum Kind<'c> {
    Null,
    Bool(bool),
    Number(Number),
    String(&'c str),
    Array(u32),
    Object(u32),
}

impl<'c> ValueRef<'c> {
    /// The value a ref in `chunk` names.
    pub(crate) fn at(chunk: &'c Chunk<'c>, reference: u32) -> ValueRef<'c> {
        let (chunk, index) = chunk.resolve(reference);
        ValueRef { chunk, index }
    }

    pub fn kind(self) -> Kind<'c> {
        let (tag, a, b) = self.chunk.value(self.index);
        let bits = u64::from(a) | u64::from(b) << 32;
        match Tag::from_word(tag) {
            Tag::Null => Kind::Null,
            Tag::False => Kind::Bool(false),
            Tag::True => Kind::Bool(true),
            Tag::Int => Kind::Number(Number::from(bits as i64)),
            Tag::UInt => Kind::Number(Number::from(bits)),
            Tag::Float => {
                Kind::Number(Number::from_f64(f64::from_bits(bits)).unwrap_or_else(|| corrupt()))
            }
            Tag::String => Kind::String(self.chunk.string(a, b)),
            Tag::Array => Kind::Array(b),
            Tag::Object => Kind::Object(b),
        }
    }

    /// Where an array's items, or an object's entries, start, and how many there are: none for
    /// a value of another tag.
    fn span(self, of: Tag) -> (u32, u32) {
        let (tag, start, len) = self.chunk.value(self.index);
        match Tag::from_word(tag) == of {
            true => (start, len),
            false => (0, 0),
        }
    }

    /// Whether this is the very value `other` is.
    pub fn ptr_eq(self, other: ValueRef) -> bool {
        self.index == other.index && self.chunk.ptr_eq(other.chunk)
    }

    /// An identity for the value: its chunk's, and its index there.
    pub fn id(self) -> (usize, u32) {
        (self.chunk.bytes().as_ptr() as usize, self.index)
    }

    pub fn is_null(self) -> bool {
        matches!(self.kind(), Kind::Null)
    }

    pub fn as_str(self) -> Option<&'c str> {
        match self.kind() {
            Kind::String(string) => Some(string),
            _ => None,
        }
    }

    pub fn as_bool(self) -> Option<bool> {
        match self.kind() {
            Kind::Bool(boolean) => Some(boolean),
            _ => None,
        }
    }

    pub fn as_f64(self) -> Option<f64> {
        match self.kind() {
            Kind::Number(number) => number.as_f64(),
            _ => None,
        }
    }

    pub fn is_object(self) -> bool {
        matches!(self.kind(), Kind::Object(..))
    }

    pub fn is_array(self) -> bool {
        matches!(self.kind(), Kind::Array(..))
    }

    /// An array's or object's number of items, and 0 for anything else.
    pub fn len(self) -> usize {
        match self.kind() {
            Kind::Array(len) | Kind::Object(len) => len as usize,
            _ => 0,
        }
    }

    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// An array's items; none for anything else.
    pub fn items(self) -> impl DoubleEndedIterator<Item = ValueRef<'c>> + ExactSizeIterator {
        let (start, len) = self.span(Tag::Array);
        (start..start + len).map(move |at| ValueRef {
            chunk: self.chunk,
            index: self.chunk.element(self.index, at),
        })
    }

    /// An object's keys and values, in order; none for anything else.
    pub fn entries(
        self,
    ) -> impl DoubleEndedIterator<Item = (&'c str, ValueRef<'c>)> + ExactSizeIterator {
        let (start, len) = self.span(Tag::Object);
        (start..start + len).map(move |at| {
            let (key, index) = self.chunk.entry(self.index, at);
            (
                key,
                ValueRef {
                    chunk: self.chunk,
                    index,
                },
            )
        })
    }

    pub fn keys(self) -> impl Iterator<Item = &'c str> {
        self.entries().map(|(key, _)| key)
    }

    /// An object's keys, as bytes, and values.
    pub(crate) fn entries_bytes(self) -> impl Iterator<Item = (&'c [u8], ValueRef<'c>)> {
        let (start, len) = self.span(Tag::Object);
        (start..start + len).map(move |at| {
            let (key, index) = self.chunk.entry_bytes(self.index, at);
            (
                key,
                ValueRef {
                    chunk: self.chunk,
                    index,
                },
            )
        })
    }

    /// An object's value for `key`.
    pub fn get(self, key: &str) -> Option<ValueRef<'c>> {
        self.entries_bytes()
            .find(|(name, _)| *name == key.as_bytes())
            .map(|(_, value)| value)
    }

    pub fn to_value(self) -> Value {
        match self.kind() {
            Kind::Null => Value::Null,
            Kind::Bool(boolean) => Value::Bool(boolean),
            Kind::Number(number) => Value::Number(number),
            Kind::String(string) => Value::String(string.into()),
            Kind::Array(..) => Value::Array(
                self.items()
                    .map(|item| stack::grow(|| item.to_value()))
                    .collect(),
            ),
            Kind::Object(..) => Value::Object(self.to_map()),
        }
    }

    /// An object's entries as a map; empty for anything else.
    pub fn to_map(self) -> Map {
        self.entries()
            .map(|(key, value)| (key.into(), stack::grow(|| value.to_value())))
            .collect()
    }

    /// `typeof`, `null` being `"null"`.
    pub fn type_of(self) -> &'static str {
        match Tag::from_word(self.chunk.value(self.index).0) {
            Tag::Null => "null",
            Tag::False | Tag::True => "boolean",
            Tag::Int | Tag::UInt | Tag::Float => "number",
            Tag::String => "string",
            Tag::Array | Tag::Object => "object",
        }
    }

    /// `JSON.stringify(value)`.
    pub fn write_json(self, out: &mut String) {
        match self.kind() {
            Kind::Null => out.push_str("null"),
            Kind::Bool(boolean) => out.push_str(if boolean { "true" } else { "false" }),
            Kind::Number(number) => write_number(out, &number),
            Kind::String(string) => write_string(out, string),
            Kind::Array(..) => {
                out.push('[');
                for (index, item) in self.items().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    stack::grow(|| item.write_json(out));
                }
                out.push(']');
            }
            Kind::Object(..) => {
                out.push('{');
                for (index, (key, value)) in self.entries().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_string(out, key);
                    out.push(':');
                    stack::grow(|| value.write_json(out));
                }
                out.push('}');
            }
        }
    }

    /// `compareDeep` with a value outside the chunk.
    pub fn equals(self, other: &Value) -> bool {
        match (self.kind(), other) {
            (Kind::Null, Value::Null) => true,
            (Kind::Bool(a), Value::Bool(b)) => a == *b,
            (Kind::Number(a), Value::Number(b)) => js::same_number(&a, b),
            (Kind::String(a), Value::String(b)) => a == b,
            (Kind::Array(..), Value::Array(items)) => {
                self.len() == items.len()
                    && self
                        .items()
                        .zip(items)
                        .all(|(a, b)| stack::grow(|| a.equals(b)))
            }
            (Kind::Object(..), Value::Object(map)) => self.equals_map(map),
            _ => false,
        }
    }

    /// `compareDeep` of an object with `map`: the same keys, with deeply equal values.
    pub fn equals_map(self, map: &Map) -> bool {
        self.is_object()
            && self.len() == map.len()
            && self
                .entries()
                .all(|(key, value)| map.get(key).is_some_and(|other| value.equals(other)))
    }
}

/// `compareDeep` of two values in chunks: arrays and objects by their contents, everything else
/// with `===`.
pub(crate) fn value_equals(a: ValueRef, b: ValueRef) -> bool {
    if a.ptr_eq(b) {
        return true;
    }
    match (a.kind(), b.kind()) {
        (Kind::Null, Kind::Null) => true,
        (Kind::Bool(a), Kind::Bool(b)) => a == b,
        (Kind::Number(a), Kind::Number(b)) => js::same_number(&a, &b),
        (Kind::String(a), Kind::String(b)) => a == b,
        (Kind::Array(a_len), Kind::Array(b_len)) => {
            a_len == b_len
                && a.items()
                    .zip(b.items())
                    .all(|(a, b)| stack::grow(|| value_equals(a, b)))
        }
        (Kind::Object(a_len), Kind::Object(b_len)) => {
            a_len == b_len
                && a.entries().all(|(key, value)| {
                    b.get(key)
                        .is_some_and(|other| stack::grow(|| value_equals(value, other)))
                })
        }
        _ => false,
    }
}

impl fmt::Debug for ValueRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut out = String::new();
        self.write_json(&mut out);
        f.write_str(&out)
    }
}
