//! The fields of a node's or mark's JSON, as `toJSON` makes them, read from the chunk for each
//! form the JSON is written in.

use super::mark::Mark;
use super::node::Node;
use super::view::{MarkRef, NodeRef, SetRef, TextRef};
use crate::chunk::ValueRef;
use crate::js::json;
use crate::json::{Map, Value};
use crate::stack;

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
    fn fields(self, field: impl FnMut(Field<'c>));

    fn field_count(self) -> usize;
}

impl<'c> Fields<'c> for NodeRef<'c> {
    #[inline]
    fn fields(self, mut field: impl FnMut(Field<'c>)) {
        field(Field::Type(self.node_type().name()));
        let attrs = self.attrs();
        if !attrs.is_empty() {
            field(Field::Attrs(attrs));
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
            !self.attrs().is_empty(),
            self.content_size() > 0,
            !self.marks().is_empty(),
            self.is_text(),
        )
    }
}

impl<'c> Fields<'c> for MarkRef<'c> {
    #[inline]
    fn fields(self, mut field: impl FnMut(Field<'c>)) {
        field(Field::Type(self.mark_type().name()));
        let attrs = self.attrs();
        if !attrs.is_empty() {
            field(Field::Attrs(attrs));
        }
    }

    fn field_count(self) -> usize {
        field_count(!self.attrs().is_empty(), false, false, false)
    }
}

/// How many fields `toJSON` writes: the type, and each other field that has something in it.
pub(crate) fn field_count(attrs: bool, content: bool, marks: bool, text: bool) -> usize {
    1 + usize::from(attrs) + usize::from(content) + usize::from(marks) + usize::from(text)
}

impl Node<'_> {
    pub fn to_json(&self) -> Value {
        to_value(self.view())
    }

    /// `JSON.stringify(node.toJSON())`. Text keeps a lone surrogate, which a JSON value can't.
    pub fn to_json_string(&self) -> String {
        let mut out = String::new();
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
    let mut object = Map::with_capacity(fields.field_count());
    fields.fields(|field| object.push(field.key().into(), field_value(field)));
    Value::Object(object)
}

fn field_value(field: Field) -> Value {
    match field {
        Field::Type(name) => Value::String(name.into()),
        Field::Attrs(attrs) => attrs.to_value(),
        Field::Content(node) => Value::Array(
            node.children()
                .map(|child| stack::grow(|| to_value(child)))
                .collect(),
        ),
        Field::Marks(marks) => Value::Array(marks.iter().map(to_value).collect()),
        Field::Text(text) => Value::String(text.to_string_lossy().into_owned()),
    }
}

pub(crate) fn write_json<'c>(out: &mut String, fields: impl Fields<'c>) {
    out.push('{');
    let mut first = true;
    fields.fields(|field| {
        if !std::mem::take(&mut first) {
            out.push(',');
        }
        json::write_string(out, field.key());
        out.push(':');
        match field {
            Field::Type(name) => json::write_string(out, name),
            Field::Attrs(attrs) => attrs.write_json(out),
            Field::Content(node) => {
                out.push('[');
                for (index, child) in node.children().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    stack::grow(|| write_json(out, child));
                }
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
