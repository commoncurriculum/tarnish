//! The fields of a node's or mark's JSON, as `toJSON` makes them, read from the chunk for each
//! form the JSON is written in.

use super::mark::Mark;
use super::node::Node;
use super::view::{MarkRef, NodeRef, SetRef, TextRef};
use crate::chunk::ValueRef;
use crate::js::json;
use crate::js::stack;
use crate::json::{Key, Map, Value};

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

    /// The key as `JSON.stringify` writes it, with its colon, and the comma before it unless it
    /// is the first.
    fn written_key(self) -> &'static str {
        match self {
            Field::Type(_) => "\"type\":",
            Field::Attrs(_) => ",\"attrs\":",
            Field::Content(_) => ",\"content\":",
            Field::Marks(_) => ",\"marks\":",
            Field::Text(_) => ",\"text\":",
        }
    }
}

/// A node or mark, as the fields of its JSON.
pub trait Fields<'c>: Copy {
    /// Each field `toJSON` writes, in its order.
    fn fields(self, field: impl FnMut(Field<'c>));

    fn field_count(self) -> usize;
}

impl<'c> Fields<'c> for NodeRef<'c> {
    #[inline]
    fn fields(self, mut field: impl FnMut(Field<'c>)) {
        let node_type = self.node_type();
        field(Field::Type(node_type.name()));
        if !node_type.data().attrs.is_empty() {
            field(Field::Attrs(self.attrs()));
        }
        if self.content_size() > 0 {
            field(Field::Content(self));
        }
        let marks = self.marks();
        if !marks.is_empty() {
            field(Field::Marks(marks));
        }
        if let Some(text) = self.text() {
            field(Field::Text(text));
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

impl<'c> Fields<'c> for MarkRef<'c> {
    #[inline]
    fn fields(self, mut field: impl FnMut(Field<'c>)) {
        let mark_type = self.mark_type();
        field(Field::Type(mark_type.name()));
        if !mark_type.data().attrs.is_empty() {
            field(Field::Attrs(self.attrs()));
        }
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
    pub fn to_json(&self) -> Value {
        to_value(self.view())
    }

    /// `JSON.stringify(node.toJSON())`. Text keeps a lone surrogate, which a JSON value can't.
    pub fn to_json_string(&self) -> String {
        // Room for the JSON of a document of short paragraphs, so that writing it grows the
        // string a time or two rather than a dozen.
        let mut out = String::with_capacity(self.node_size().saturating_mul(4).max(64));
        write_json(&mut out, self.view());
        out
    }
}

impl Mark<'_> {
    pub fn to_json(&self) -> Value {
        to_value(self.view())
    }
}

pub(crate) fn to_value<'c>(fields: impl Fields<'c>) -> Value {
    // The first push makes room for four entries, which is as many as a node of most schemas has,
    // without reading the fields twice to count them.
    let mut object = Map::new();
    fields.fields(|field| object.push(Key::const_new(field.key()), field_value(field)));
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

// bounded: a mark's fields hold no content or marks, so writing one goes a single level down;
// content grows the stack
pub(crate) fn write_json<'c>(out: &mut String, fields: impl Fields<'c>) {
    out.push('{');
    // The type is first, so each key but its writes the comma before it.
    fields.fields(|field| {
        out.push_str(field.written_key());
        match field {
            Field::Type(name) => json::write_string(out, name),
            Field::Attrs(attrs) => attrs.write_json(out),
            Field::Content(node) => {
                out.push('[');
                // One check of the stack for the level, rather than one for each child.
                stack::grow(|| {
                    for (index, child) in node.children().enumerate() {
                        if index > 0 {
                            out.push(',');
                        }
                        write_json(out, child);
                    }
                });
                out.push(']');
            }
            Field::Marks(marks) => {
                out.push('[');
                for (index, mark) in marks.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_json(out, mark);
                }
                out.push(']');
            }
            Field::Text(text) => text.write_json(out),
        }
    });
    out.push('}');
}
