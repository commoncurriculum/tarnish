//! The JSON `toJSON` makes of a node, mark, fragment, slice or step, read in place from what
//! holds it: no object is built until one is asked for.

use std::borrow::Cow;
use std::fmt;

use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

use super::fields::{Field, Fields, Json};
use super::fragment::Fragment;
use super::replace::Slice;
use super::view::{MarkRef, NodeRef, SetRef, TextRef};
use crate::chunk::{Kind, ValueRef};
use crate::js::{self, stack};
use crate::json::{Key, Number, Value};
use crate::transform::Step;

/// JSON as `toJSON` makes it, or a part of it, read where it is held. It reads, compares and
/// serializes as the [`Value`] that [`to_value`](Self::to_value) copies it to.
#[derive(Clone, Copy)]
pub struct JsonRef<'c>(Part<'c>);

#[derive(Clone, Copy)]
enum Part<'c> {
    Node(NodeRef<'c>),
    Mark(MarkRef<'c>),
    /// Attributes, or a value in them.
    Value(ValueRef<'c>),
    /// A type's name, a step's type or another string.
    Str(&'c str),
    Text(TextRef<'c>),
    /// The node whose children the content is.
    Content(NodeRef<'c>),
    Marks(SetRef<'c>),
    Fragment(&'c Fragment<'c>),
    Slice(&'c Slice<'c>),
    Step(&'c Step<'c>),
    Number(Number),
    Bool(bool),
    /// A value held as it is, such as an attribute a step sets.
    Json(&'c Value),
}

/// What a [`JsonRef`] is: an object or array with its length, or a value.
#[derive(Clone, Debug, PartialEq)]
pub enum JsonKind<'c> {
    Null,
    Bool(bool),
    Number(Number),
    /// A string; text with a lone surrogate has U+FFFD in its place, as in a [`Value`].
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

    pub(crate) fn fragment(fragment: &'c Fragment<'c>) -> JsonRef<'c> {
        JsonRef(Part::Fragment(fragment))
    }

    pub(crate) fn slice(slice: &'c Slice<'c>) -> JsonRef<'c> {
        JsonRef(Part::Slice(slice))
    }

    // bounded: calls a custom step's own `to_json`, which doesn't come back here
    pub(crate) fn step(step: &'c Step<'c>) -> JsonRef<'c> {
        match step {
            Step::Custom(step) => step.to_json(),
            step => JsonRef(Part::Step(step)),
        }
    }

    /// A value as it is, as a [`CustomStep`](crate::transform::CustomStep) gives its JSON.
    pub fn value(value: &'c Value) -> JsonRef<'c> {
        JsonRef(Part::Json(value))
    }

    fn field(field: Field<'c>) -> JsonRef<'c> {
        JsonRef(match field {
            Field::Type(name) => Part::Str(name),
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
            Part::Str(string) => JsonKind::String(Cow::Borrowed(string)),
            Part::Text(text) => JsonKind::String(text.to_string_lossy()),
            Part::Content(node) => JsonKind::Array(node.child_count() as usize),
            Part::Marks(marks) => JsonKind::Array(marks.len()),
            Part::Fragment(fragment) if fragment.child_count() == 0 => JsonKind::Null,
            Part::Fragment(fragment) => JsonKind::Array(fragment.child_count()),
            Part::Slice(slice) if slice.content().size() == 0 => JsonKind::Null,
            Part::Slice(_) | Part::Step(_) => JsonKind::Object(self.entries().count()),
            Part::Number(number) => JsonKind::Number(number),
            Part::Bool(boolean) => JsonKind::Bool(boolean),
            Part::Json(value) => match value {
                Value::Null => JsonKind::Null,
                Value::Bool(boolean) => JsonKind::Bool(*boolean),
                Value::Number(number) => JsonKind::Number(*number),
                Value::String(string) => JsonKind::String(Cow::Borrowed(string)),
                Value::Array(items) => JsonKind::Array(items.len()),
                Value::Object(map) => JsonKind::Object(map.len()),
            },
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

    /// A string; text with a lone surrogate has U+FFFD in its place, as in a [`Value`].
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
            Part::Json(value) => value.as_object()?.get(key).map(JsonRef::value),
            Part::Slice(_) | Part::Step(_) => self
                .entries()
                .find(|&(name, _)| name == key)
                .map(|(_, value)| value),
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
            Part::Fragment(fragment) => (index < fragment.child_count())
                .then(|| JsonRef::node(fragment.refs().nth(index).expect("a child"))),
            Part::Json(value) => value.as_array()?.get(index).map(JsonRef::value),
            _ => None,
        }
    }

    /// An array's items; none for anything else.
    pub fn items(self) -> impl ExactSizeIterator<Item = JsonRef<'c>> {
        match self.0 {
            Part::Content(node) => Items::Content(node.children()),
            Part::Marks(marks) => Items::Marks(marks.iter()),
            Part::Value(value) if value.is_array() => Items::Value(value.items()),
            Part::Fragment(fragment) => Items::Fragment(fragment.refs()),
            Part::Json(Value::Array(items)) => Items::Json(items.iter()),
            _ => Items::None,
        }
    }

    /// An object's keys and values, in the order `toJSON` writes them; none for anything else.
    pub fn entries(self) -> impl Iterator<Item = (&'c str, JsonRef<'c>)> {
        match self.0 {
            Part::Node(node) => Entries::Node(node.fields()),
            Part::Mark(mark) => Entries::Mark(mark.fields()),
            Part::Value(value) if value.is_object() => Entries::Value(value.entries()),
            Part::Json(Value::Object(map)) => Entries::Json(map.iter()),
            Part::Slice(slice) if slice.content().size() > 0 => Entries::Listed(Listed {
                part: self.0,
                next: 0,
            }),
            Part::Step(_) => Entries::Listed(Listed {
                part: self.0,
                next: 0,
            }),
            _ => Entries::None,
        }
    }

    /// A copy of it, for code that edits it.
    pub fn to_value(self) -> Value {
        match self.0 {
            Part::Node(node) => super::fields::to_value(node),
            Part::Mark(mark) => super::fields::to_value(mark),
            Part::Value(value) => value.to_value(),
            Part::Str(string) => Value::String(string.into()),
            Part::Text(text) => Value::String(text.to_string_lossy().as_ref().into()),
            Part::Content(node) => Value::Array(stack::grow(|| {
                node.children().map(super::fields::to_value).collect()
            })),
            Part::Marks(marks) => Value::Array(marks.iter().map(super::fields::to_value).collect()),
            Part::Fragment(fragment) if fragment.child_count() == 0 => Value::Null,
            Part::Fragment(fragment) => Value::Array(stack::grow(|| {
                fragment.refs().map(super::fields::to_value).collect()
            })),
            Part::Slice(slice) if slice.content().size() == 0 => Value::Null,
            Part::Slice(_) | Part::Step(_) => Value::Object(
                self.entries()
                    .map(|(key, value)| (key.into(), stack::grow(|| value.to_value())))
                    .collect(),
            ),
            Part::Number(number) => Value::Number(number),
            Part::Bool(boolean) => Value::Bool(boolean),
            Part::Json(value) => value.clone(),
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

/// A slice's or step's fields, each listed as it is asked for, which keeps the iterator small.
struct Listed<'c> {
    part: Part<'c>,
    next: usize,
}

impl<'c> Iterator for Listed<'c> {
    type Item = (&'c str, JsonRef<'c>);

    fn next(&mut self) -> Option<(&'c str, JsonRef<'c>)> {
        let fields = match self.part {
            Part::Slice(slice) => slice_entries(slice),
            Part::Step(step) => step_entries(step),
            _ => [None; 8],
        };
        while self.next < fields.len() {
            self.next += 1;
            if let Some(field) = fields[self.next - 1] {
                return Some(field);
            }
        }
        None
    }
}

fn number<'c>(number: usize) -> JsonRef<'c> {
    JsonRef(Part::Number(number.into()))
}

/// Room for the most fields a step's JSON has.
type Fields8<'c> = [Option<(&'c str, JsonRef<'c>)>; 8];

/// A slice's fields, as `Slice.toJSON` writes them.
fn slice_entries<'c>(slice: &'c Slice<'c>) -> Fields8<'c> {
    [
        Some(("content", JsonRef::fragment(slice.content()))),
        (slice.open_start() > 0).then(|| ("openStart", number(slice.open_start()))),
        (slice.open_end() > 0).then(|| ("openEnd", number(slice.open_end()))),
        None,
        None,
        None,
        None,
        None,
    ]
}

/// A step's fields, as its `toJSON` writes them.
fn step_entries<'c>(step: &'c Step<'c>) -> Fields8<'c> {
    let step_type = Some(("stepType", JsonRef(Part::Str(step.json_id()))));
    let slice = |slice: &'c Slice<'c>| (slice.size() > 0).then(|| ("slice", JsonRef::slice(slice)));
    let structure = |structure: bool| structure.then_some(("structure", JsonRef(Part::Bool(true))));
    let attr = |attr: &'c String, value: &'c Option<Value>| {
        [
            Some(("attr", JsonRef(Part::Str(attr)))),
            value.as_ref().map(|value| ("value", JsonRef::value(value))),
        ]
    };
    match step {
        Step::Replace {
            from,
            to,
            slice: content,
            structure: is_structure,
        } => [
            step_type,
            Some(("from", number(*from))),
            Some(("to", number(*to))),
            slice(content),
            structure(*is_structure),
            None,
            None,
            None,
        ],
        Step::ReplaceAround {
            from,
            to,
            gap_from,
            gap_to,
            slice: content,
            insert,
            structure: is_structure,
        } => [
            step_type,
            Some(("from", number(*from))),
            Some(("to", number(*to))),
            Some(("gapFrom", number(*gap_from))),
            Some(("gapTo", number(*gap_to))),
            Some(("insert", number(*insert))),
            slice(content),
            structure(*is_structure),
        ],
        Step::Mark { from, to, mark, .. } => [
            step_type,
            Some(("mark", mark.to_json())),
            Some(("from", number(*from))),
            Some(("to", number(*to))),
            None,
            None,
            None,
            None,
        ],
        Step::NodeMark { pos, mark, .. } => [
            step_type,
            Some(("pos", number(*pos))),
            Some(("mark", mark.to_json())),
            None,
            None,
            None,
            None,
            None,
        ],
        Step::Attr {
            pos,
            attr: name,
            value,
        } => {
            let [name, value] = attr(name, value);
            [
                step_type,
                Some(("pos", number(*pos))),
                name,
                value,
                None,
                None,
                None,
                None,
            ]
        }
        Step::DocAttr { attr: name, value } => {
            let [name, value] = attr(name, value);
            [step_type, name, value, None, None, None, None, None]
        }
        // Read through `JsonRef::step`, which gives a custom step's own JSON.
        Step::Custom(_) => [None; 8],
    }
}

/// An object's entries, from whichever part holds them.
enum Entries<'c, N, M, V, J> {
    Node(N),
    Mark(M),
    Value(V),
    Json(J),
    Listed(Listed<'c>),
    None,
}

impl<'c, N, M, V, J> Iterator for Entries<'c, N, M, V, J>
where
    N: Iterator<Item = Field<'c>>,
    M: Iterator<Item = Field<'c>>,
    V: Iterator<Item = (&'c str, ValueRef<'c>)>,
    J: Iterator<Item = (&'c Key, &'c Value)>,
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
            Entries::Json(entries) => entries
                .next()
                .map(|(key, value)| (key.as_str(), JsonRef::value(value))),
            Entries::Listed(entries) => entries.next(),
            Entries::None => None,
        }
    }
}

/// An array's items, from whichever part holds them.
enum Items<'c, C, F, M, V> {
    Content(C),
    Fragment(F),
    Marks(M),
    Value(V),
    Json(std::slice::Iter<'c, Value>),
    None,
}

impl<'c, C, F, M, V> Iterator for Items<'c, C, F, M, V>
where
    C: ExactSizeIterator<Item = NodeRef<'c>>,
    F: ExactSizeIterator<Item = NodeRef<'c>>,
    M: ExactSizeIterator<Item = MarkRef<'c>>,
    V: ExactSizeIterator<Item = ValueRef<'c>>,
{
    type Item = JsonRef<'c>;

    #[inline]
    fn next(&mut self) -> Option<JsonRef<'c>> {
        match self {
            Items::Content(nodes) => nodes.next().map(JsonRef::node),
            Items::Fragment(nodes) => nodes.next().map(JsonRef::node),
            Items::Marks(marks) => marks.next().map(JsonRef::mark),
            Items::Value(items) => items.next().map(|item| JsonRef(Part::Value(item))),
            Items::Json(items) => items.next().map(JsonRef::value),
            Items::None => None,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = match self {
            Items::Content(nodes) => nodes.len(),
            Items::Fragment(nodes) => nodes.len(),
            Items::Marks(marks) => marks.len(),
            Items::Value(items) => items.len(),
            Items::Json(items) => items.len(),
            Items::None => 0,
        };
        (len, Some(len))
    }
}

impl<'c, C, F, M, V> ExactSizeIterator for Items<'c, C, F, M, V>
where
    C: ExactSizeIterator<Item = NodeRef<'c>>,
    F: ExactSizeIterator<Item = NodeRef<'c>>,
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
            Part::Str(string) => serializer.serialize_str(string),
            Part::Text(text) => text.serialize(serializer),
            Part::Content(node) => {
                stack::grow(|| serializer.collect_seq(node.children().map(Json)))
            }
            Part::Marks(marks) => serializer.collect_seq(marks.iter().map(Json)),
            Part::Fragment(fragment) if fragment.child_count() == 0 => serializer.serialize_unit(),
            Part::Fragment(fragment) => {
                stack::grow(|| serializer.collect_seq(fragment.refs().map(Json)))
            }
            Part::Slice(slice) if slice.content().size() == 0 => serializer.serialize_unit(),
            Part::Slice(_) | Part::Step(_) => {
                let mut map = serializer.serialize_map(None)?;
                for (key, value) in self.entries() {
                    stack::grow(|| map.serialize_entry(key, &value))?;
                }
                map.end()
            }
            Part::Number(number) => Value::Number(number).serialize(serializer),
            Part::Bool(boolean) => serializer.serialize_bool(boolean),
            Part::Json(value) => value.serialize(serializer),
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

/// Equal as [`Value`]s are, without copying either.
impl PartialEq for JsonRef<'_> {
    fn eq(&self, other: &JsonRef) -> bool {
        match (self.kind(), other.kind()) {
            (JsonKind::Array(a), JsonKind::Array(b)) => {
                a == b
                    && self
                        .items()
                        .zip(other.items())
                        .all(|(a, b)| stack::grow(|| a == b))
            }
            (JsonKind::Object(a), JsonKind::Object(b)) => {
                a == b
                    && self
                        .entries()
                        .all(|(key, a)| other.get(key).is_some_and(|b| stack::grow(|| a == b)))
            }
            (a, b) => a == b,
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

/// `json!` takes the JSON in, as the value it copies out.
impl crate::js::json::IntoValue for JsonRef<'_> {
    fn into_value(self) -> Value {
        self.to_value()
    }
}

impl From<JsonRef<'_>> for Value {
    fn from(json: JsonRef) -> Value {
        json.to_value()
    }
}
