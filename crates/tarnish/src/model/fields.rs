//! The fields of a node's or mark's JSON, as `toJSON` makes them, read from the chunk for each
//! form the JSON is written in.

use super::json_ref::JsonRef;
use super::mark::Mark;
use super::node::Node;
use super::view::{MarkRef, NodeRef, SetRef, TextRef};
use crate::chunk::ValueRef;
use crate::js::json;
use crate::js::stack;
use crate::json::{Key, Map, Value};
use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};

#[derive(Clone, Copy)]
pub enum Field<'c> {
    Type(&'c str),
    Attrs(ValueRef<'c>),
    /// The node whose children the content is.
    Content(NodeRef<'c>),
    Marks(SetRef<'c>),
    Text(TextRef<'c>),
}

impl Field<'_> {
    pub fn key(self) -> &'static str {
        match self {
            Field::Type(_) => "type",
            Field::Attrs(_) => "attrs",
            Field::Content(_) => "content",
            Field::Marks(_) => "marks",
            Field::Text(_) => "text",
        }
    }
}

/// A node or mark, as the fields of its JSON.
pub trait Fields<'c>: Copy {
    /// Each field `toJSON` writes, in its order.
    fn fields(self) -> impl Iterator<Item = Field<'c>>;

    fn field_count(self) -> usize;
}

impl<'c> Fields<'c> for NodeRef<'c> {
    #[inline]
    fn fields(self) -> impl Iterator<Item = Field<'c>> {
        NodeFields {
            node: self,
            next: 0,
        }
    }

    fn field_count(self) -> usize {
        field_count(
            !self.node_type().data().attrs.is_empty(),
            self.content_size() > 0,
            !self.marks().is_empty(),
            self.is_text(),
        )
    }
}

/// A node's fields, each read as it is asked for, which keeps the iterator as small as the node.
struct NodeFields<'c> {
    node: NodeRef<'c>,
    /// The field to look at next, in `toJSON`'s order.
    next: u8,
}

impl<'c> Iterator for NodeFields<'c> {
    type Item = Field<'c>;

    #[inline]
    fn next(&mut self) -> Option<Field<'c>> {
        let node = self.node;
        loop {
            self.next += 1;
            let field = match self.next {
                1 => Some(Field::Type(node.node_type().name())),
                2 => {
                    (!node.node_type().data().attrs.is_empty()).then(|| Field::Attrs(node.attrs()))
                }
                3 => (node.content_size() > 0).then_some(Field::Content(node)),
                4 => Some(node.marks())
                    .filter(|marks| !marks.is_empty())
                    .map(Field::Marks),
                5 => node.text().map(Field::Text),
                _ => return None,
            };
            if field.is_some() {
                return field;
            }
        }
    }
}

impl<'c> Fields<'c> for MarkRef<'c> {
    #[inline]
    fn fields(self) -> impl Iterator<Item = Field<'c>> {
        let mark_type = self.mark_type();
        let attrs = (!mark_type.data().attrs.is_empty()).then(|| Field::Attrs(self.attrs()));
        std::iter::once(Field::Type(mark_type.name())).chain(attrs)
    }

    fn field_count(self) -> usize {
        field_count(
            !self.mark_type().data().attrs.is_empty(),
            false,
            false,
            false,
        )
    }
}

/// How many fields `toJSON` writes: the type, the attributes where the type has any, as
/// ProseMirror's hold each of the type's names, `undefined` ones too, and each other field that
/// has something in it.
pub(crate) fn field_count(attrs: bool, content: bool, marks: bool, text: bool) -> usize {
    1 + usize::from(attrs) + usize::from(content) + usize::from(marks) + usize::from(text)
}

impl Node<'_> {
    /// `node.toJSON()`, read in place from the document: nothing is built or copied until
    /// [`JsonRef::to_value`] copies it out for code that edits it.
    pub fn to_json(&self) -> JsonRef<'_> {
        JsonRef::node(self.view())
    }

    /// `JSON.stringify(node.toJSON())`. Text keeps a lone surrogate, which a JSON value can't.
    pub fn to_json_string(&self) -> String {
        // Room for the JSON of a document of short paragraphs, so that writing it grows the
        // string a time or two rather than a dozen.
        let capacity = self.node_size().saturating_mul(4).max(64);
        json::to_string_with_capacity(&Json(self.view()), capacity)
    }
}

impl Mark<'_> {
    /// `mark.toJSON()`, read in place: nothing is built or copied.
    pub fn to_json(&self) -> JsonRef<'_> {
        JsonRef::mark(self.view())
    }
}

pub(crate) fn to_value<'c>(fields: impl Fields<'c>) -> Value {
    // The first push makes room for four entries, which is as many as a node of most schemas has,
    // without reading the fields twice to count them.
    let mut object = Map::new();
    for field in fields.fields() {
        object.push(Key::const_new(field.key()), field_value(field));
    }
    Value::Object(object)
}

fn field_value(field: Field) -> Value {
    match field {
        Field::Type(name) => Value::String(name.into()),
        Field::Attrs(attrs) => attrs.to_value(),
        // One check of the stack for the level, rather than one for each child.
        Field::Content(node) => {
            Value::Array(stack::grow(|| node.children().map(to_value).collect()))
        }
        Field::Marks(marks) => Value::Array(marks.iter().map(to_value).collect()),
        Field::Text(text) => Value::String(text.to_string_lossy().as_ref().into()),
    }
}

/// A node or mark as `toJSON` makes it, serialized as `JSON.stringify` writes it.
#[derive(Clone, Copy)]
pub struct Json<F>(pub F);

impl<'c, F: Fields<'c>> Serialize for Json<F> {
    // bounded: a mark's fields hold no content or marks, so writing one goes a single level
    // down; content grows the stack
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.field_count()))?;
        for field in self.0.fields() {
            let key = field.key();
            match field {
                Field::Type(name) => map.serialize_entry(key, name),
                Field::Attrs(attrs) => map.serialize_entry(key, &attrs),
                // One check of the stack for the level, rather than one for each child.
                Field::Content(node) => stack::grow(|| map.serialize_entry(key, &Children(node))),
                Field::Marks(marks) => map.serialize_entry(key, &Marks(marks)),
                Field::Text(text) => map.serialize_entry(key, &text),
            }?;
        }
        map.end()
    }
}

/// A node's children, as the content of its JSON.
struct Children<'c>(NodeRef<'c>);

impl Serialize for Children<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.children().map(Json))
    }
}

/// A node's marks, as the marks of its JSON.
struct Marks<'c>(SetRef<'c>);

impl Serialize for Marks<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(Json))
    }
}
