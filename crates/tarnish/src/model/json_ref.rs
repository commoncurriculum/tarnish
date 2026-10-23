//! A node's or mark's JSON, as `toJSON` makes it, read in place from the document's chunk: no
//! object is built until one is asked for.

use std::borrow::Cow;
use std::fmt;

use serde::{Serialize, Serializer};

use super::fields::{Field, Fields, Json};
use super::view::{MarkRef, NodeRef, SetRef, TextRef};
use crate::chunk::{Kind, ValueRef};
use crate::js::{self, stack};
use crate::json::{Number, Value};

/// A part of a node's or mark's JSON, read where the document holds it. It reads, compares and
/// serializes as the [`Value`] `to_json` makes, which [`to_value`](Self::to_value) copies it to.
#[derive(Clone, Copy)]
pub struct JsonRef<'c>(Part<'c>);

#[derive(Clone, Copy)]
enum Part<'c> {
    Node(NodeRef<'c>),
    Mark(MarkRef<'c>),
    /// Attributes, or a value in them.
    Value(ValueRef<'c>),
    /// A node's or mark's type.
    Name(&'c str),
    Text(TextRef<'c>),
    /// The node whose children the content is.
    Content(NodeRef<'c>),
    Marks(SetRef<'c>),
}

/// What a [`JsonRef`] is: an object or array with its length, or a value.
#[derive(Clone, Debug, PartialEq)]
pub enum JsonKind<'c> {
    Null,
    Bool(bool),
    Number(Number),
    /// A string; text with a lone surrogate has U+FFFD in its place, as in `to_json`.
    String(Cow<'c, str>),
    Array(usize),
    Object(usize),
}

impl<'c> JsonRef<'c> {
    pub(crate) fn node(node: NodeRef<'c>) -> JsonRef<'c> {
        JsonRef(Part::Node(node))
    }

    pub(crate) fn mark(mark: MarkRef<'c>) -> JsonRef<'c> {
        JsonRef(Part::Mark(mark))
    }

    fn field(field: Field<'c>) -> JsonRef<'c> {
        JsonRef(match field {
            Field::Type(name) => Part::Name(name),
            Field::Attrs(attrs) => Part::Value(attrs),
            Field::Content(node) => Part::Content(node),
            Field::Marks(marks) => Part::Marks(marks),
            Field::Text(text) => Part::Text(text),
        })
    }

    pub fn kind(self) -> JsonKind<'c> {
        match self.0 {
            Part::Node(node) => JsonKind::Object(node.field_count()),
            Part::Mark(mark) => JsonKind::Object(mark.field_count()),
            Part::Value(value) => match value.kind() {
                Kind::Null => JsonKind::Null,
                Kind::Bool(boolean) => JsonKind::Bool(boolean),
                Kind::Number(number) => JsonKind::Number(number),
                Kind::String(string) => JsonKind::String(Cow::Borrowed(string)),
                Kind::Array(len) => JsonKind::Array(len as usize),
                Kind::Object(len) => JsonKind::Object(len as usize),
            },
            Part::Name(name) => JsonKind::String(Cow::Borrowed(name)),
            Part::Text(text) => JsonKind::String(text.to_string_lossy()),
            Part::Content(node) => JsonKind::Array(node.child_count() as usize),
            Part::Marks(marks) => JsonKind::Array(marks.len()),
        }
    }

    pub fn is_null(self) -> bool {
        matches!(self.kind(), JsonKind::Null)
    }

    pub fn as_bool(self) -> Option<bool> {
        match self.kind() {
            JsonKind::Bool(boolean) => Some(boolean),
            _ => None,
        }
    }

    pub fn as_f64(self) -> Option<f64> {
        match self.kind() {
            JsonKind::Number(number) => Some(number.as_f64()),
            _ => None,
        }
    }

    /// A string; text with a lone surrogate has U+FFFD in its place, as in `to_json`.
    pub fn as_str(self) -> Option<Cow<'c, str>> {
        match self.kind() {
            JsonKind::String(string) => Some(string),
            _ => None,
        }
    }

    /// An array's length, or an object's count of keys; 0 for anything else.
    pub fn len(self) -> usize {
        match self.kind() {
            JsonKind::Array(len) | JsonKind::Object(len) => len,
            _ => 0,
        }
    }

    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// An object's value for `key`.
    pub fn get(self, key: &str) -> Option<JsonRef<'c>> {
        match self.0 {
            Part::Node(node) => field(node, key),
            Part::Mark(mark) => field(mark, key),
            Part::Value(value) if value.is_object() => {
                value.get(key).map(|v| JsonRef(Part::Value(v)))
            }
            _ => None,
        }
    }

    /// An array's item at `index`.
    pub fn at(self, index: usize) -> Option<JsonRef<'c>> {
        match self.0 {
            Part::Content(node) => (index < node.child_count() as usize)
                .then(|| JsonRef::node(node.child(index as u32))),
            Part::Marks(marks) => marks.iter().nth(index).map(JsonRef::mark),
            Part::Value(value) if value.is_array() => value
                .items()
                .nth(index)
                .map(|item| JsonRef(Part::Value(item))),
            _ => None,
        }
    }

    /// An array's items; none for anything else.
    pub fn items(self) -> impl ExactSizeIterator<Item = JsonRef<'c>> {
        match self.0 {
            Part::Content(node) => Items::Content(node.children()),
            Part::Marks(marks) => Items::Marks(marks.iter()),
            Part::Value(value) if value.is_array() => Items::Value(value.items()),
            _ => Items::None,
        }
    }

    /// An object's keys and values, in the order `toJSON` writes them; none for anything else.
    pub fn entries(self) -> impl Iterator<Item = (&'c str, JsonRef<'c>)> {
        match self.0 {
            Part::Node(node) => Entries::Node(node.fields()),
            Part::Mark(mark) => Entries::Mark(mark.fields()),
            Part::Value(value) if value.is_object() => Entries::Value(value.entries()),
            _ => Entries::None,
        }
    }

    /// The [`Value`] `to_json` makes of it.
    pub fn to_value(self) -> Value {
        match self.0 {
            Part::Node(node) => super::fields::to_value(node),
            Part::Mark(mark) => super::fields::to_value(mark),
            Part::Value(value) => value.to_value(),
            Part::Name(name) => Value::String(name.into()),
            Part::Text(text) => Value::String(text.to_string_lossy().as_ref().into()),
            Part::Content(node) => Value::Array(stack::grow(|| {
                node.children().map(super::fields::to_value).collect()
            })),
            Part::Marks(marks) => Value::Array(marks.iter().map(super::fields::to_value).collect()),
        }
    }

    /// `JSON.stringify` of it, a lone surrogate as its escape.
    pub fn to_json_string(self) -> String {
        js::json::to_string(&self)
    }
}

fn field<'c>(fields: impl Fields<'c>, key: &str) -> Option<JsonRef<'c>> {
    fields
        .fields()
        .find(|field| field.key() == key)
        .map(JsonRef::field)
}

/// An object's entries, from whichever part holds them.
enum Entries<N, M, V> {
    Node(N),
    Mark(M),
    Value(V),
    None,
}

impl<'c, N, M, V> Iterator for Entries<N, M, V>
where
    N: Iterator<Item = Field<'c>>,
    M: Iterator<Item = Field<'c>>,
    V: Iterator<Item = (&'c str, ValueRef<'c>)>,
{
    type Item = (&'c str, JsonRef<'c>);

    #[inline]
    fn next(&mut self) -> Option<(&'c str, JsonRef<'c>)> {
        let field = |field: Field<'c>| (field.key(), JsonRef::field(field));
        match self {
            Entries::Node(fields) => fields.next().map(field),
            Entries::Mark(fields) => fields.next().map(field),
            Entries::Value(entries) => entries
                .next()
                .map(|(key, value)| (key, JsonRef(Part::Value(value)))),
            Entries::None => None,
        }
    }
}

/// An array's items, from whichever part holds them.
enum Items<C, M, V> {
    Content(C),
    Marks(M),
    Value(V),
    None,
}

impl<'c, C, M, V> Iterator for Items<C, M, V>
where
    C: ExactSizeIterator<Item = NodeRef<'c>>,
    M: ExactSizeIterator<Item = MarkRef<'c>>,
    V: ExactSizeIterator<Item = ValueRef<'c>>,
{
    type Item = JsonRef<'c>;

    #[inline]
    fn next(&mut self) -> Option<JsonRef<'c>> {
        match self {
            Items::Content(nodes) => nodes.next().map(JsonRef::node),
            Items::Marks(marks) => marks.next().map(JsonRef::mark),
            Items::Value(items) => items.next().map(|item| JsonRef(Part::Value(item))),
            Items::None => None,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = match self {
            Items::Content(nodes) => nodes.len(),
            Items::Marks(marks) => marks.len(),
            Items::Value(items) => items.len(),
            Items::None => 0,
        };
        (len, Some(len))
    }
}

impl<'c, C, M, V> ExactSizeIterator for Items<C, M, V>
where
    C: ExactSizeIterator<Item = NodeRef<'c>>,
    M: ExactSizeIterator<Item = MarkRef<'c>>,
    V: ExactSizeIterator<Item = ValueRef<'c>>,
{
}

/// Serialized as `JSON.stringify` writes it.
impl Serialize for JsonRef<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Part::Node(node) => Json(node).serialize(serializer),
            Part::Mark(mark) => Json(mark).serialize(serializer),
            Part::Value(value) => value.serialize(serializer),
            Part::Name(name) => serializer.serialize_str(name),
            Part::Text(text) => text.serialize(serializer),
            Part::Content(node) => {
                stack::grow(|| serializer.collect_seq(node.children().map(Json)))
            }
            Part::Marks(marks) => serializer.collect_seq(marks.iter().map(Json)),
        }
    }
}

/// Equal to the value as [`Value`]s are equal: arrays item by item, objects with equal keys and
/// values in any order, and numbers as `===` has them.
impl PartialEq<Value> for JsonRef<'_> {
    fn eq(&self, other: &Value) -> bool {
        match (self.kind(), other) {
            (JsonKind::Null, Value::Null) => true,
            (JsonKind::Bool(a), Value::Bool(b)) => a == *b,
            (JsonKind::Number(a), Value::Number(b)) => a == *b,
            (JsonKind::String(a), Value::String(b)) => *a == **b,
            (JsonKind::Array(len), Value::Array(items)) => {
                len == items.len()
                    && self
                        .items()
                        .zip(items)
                        .all(|(a, b)| stack::grow(|| a == *b))
            }
            (JsonKind::Object(len), Value::Object(map)) => {
                len == map.len()
                    && self
                        .entries()
                        .all(|(key, a)| map.get(key).is_some_and(|b| stack::grow(|| a == *b)))
            }
            _ => false,
        }
    }
}

impl PartialEq<JsonRef<'_>> for Value {
    fn eq(&self, other: &JsonRef) -> bool {
        other == self
    }
}

impl fmt::Display for JsonRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.to_json_string())
    }
}

impl fmt::Debug for JsonRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.to_json_string())
    }
}
