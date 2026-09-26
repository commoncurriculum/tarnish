//! The fields of a node's or mark's JSON, as `toJSON` makes them. The JSON value, its text, and
//! the Erlang term each write them.

use super::mark::Mark;
use super::node::Node;
use crate::js::json;
use crate::json::{Map, Value};
use crate::stack;
use crate::text::Text;

#[derive(Clone, Copy)]
pub enum Field<'a> {
    Type(&'a str),
    Attrs(&'a Map),
    Content(&'a [Node]),
    Marks(&'a [Mark]),
    Text(&'a Text),
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
pub trait Fields {
    /// Each field `toJSON` writes, in its order.
    fn fields<'a>(&'a self, field: impl FnMut(Field<'a>));

    fn field_count(&self) -> usize {
        let mut count = 0;
        self.fields(|_| count += 1);
        count
    }
}

impl Fields for Node {
    #[inline]
    fn fields<'a>(&'a self, mut field: impl FnMut(Field<'a>)) {
        field(Field::Type(self.node_type().name()));
        if !self.attrs().is_empty() {
            field(Field::Attrs(self.attrs()));
        }
        if self.child_count() > 0 {
            field(Field::Content(self.children()));
        }
        if !self.marks().is_empty() {
            field(Field::Marks(self.marks()));
        }
        if let Some(text) = self.text() {
            field(Field::Text(text));
        }
    }

    fn field_count(&self) -> usize {
        1 + usize::from(!self.attrs().is_empty())
            + usize::from(self.child_count() > 0)
            + usize::from(!self.marks().is_empty())
            + usize::from(self.text().is_some())
    }
}

impl Fields for Mark {
    #[inline]
    fn fields<'a>(&'a self, mut field: impl FnMut(Field<'a>)) {
        field(Field::Type(self.mark_type().name()));
        if !self.attrs().is_empty() {
            field(Field::Attrs(self.attrs()));
        }
    }

    fn field_count(&self) -> usize {
        1 + usize::from(!self.attrs().is_empty())
    }
}

impl Node {
    pub fn to_json(&self) -> Value {
        to_value(self)
    }

    /// `JSON.stringify(node.toJSON())`. Text keeps a lone surrogate, which a JSON value can't.
    pub fn to_json_string(&self) -> String {
        let mut out = String::new();
        write_json(&mut out, self);
        out
    }
}

impl Mark {
    pub fn to_json(&self) -> Value {
        to_value(self)
    }
}

fn to_value(fields: &impl Fields) -> Value {
    let mut object = Map::with_capacity(fields.field_count());
    fields.fields(|field| object.push(field.key().into(), field_value(field)));
    Value::Object(object)
}

fn field_value(field: Field) -> Value {
    match field {
        Field::Type(name) => Value::String(name.into()),
        Field::Attrs(attrs) => Value::Object(attrs.clone()),
        Field::Content(children) => Value::Array(
            children
                .iter()
                .map(|child| stack::grow(|| child.to_json()))
                .collect(),
        ),
        Field::Marks(marks) => Value::Array(marks.iter().map(Mark::to_json).collect()),
        Field::Text(text) => Value::String(text.to_string_lossy().into_owned()),
    }
}

fn write_json(out: &mut String, fields: &impl Fields) {
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
            Field::Attrs(attrs) => json::write_object(out, attrs),
            Field::Content(children) => write_list(out, children, |out, child| {
                stack::grow(|| write_json(out, child))
            }),
            Field::Marks(marks) => write_list(out, marks, write_json),
            Field::Text(text) => text.write_json(out),
        }
    });
    out.push('}');
}

fn write_list<T>(out: &mut String, items: &[T], mut write: impl FnMut(&mut String, &T)) {
    out.push('[');
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write(out, item);
    }
    out.push(']');
}
