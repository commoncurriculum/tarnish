//! Reading a node straight from JSON text, as `fromJSON(JSON.parse(text))` does, without
//! building a [`Value`] of the whole document first: simd-json lays the text out as a tape, one
//! flat list of its values, whose strings the reader borrows.
//!
//! The tape reads numbers as `JSON.parse` does, keeps an object's keys in order, and refuses
//! what `JSON.parse` refuses. Where it can't answer as `JSON.parse` would, the text is read
//! through [`json::from_str`] instead: an escaped surrogate, which the tape can't hold alone,
//! and a number the tape won't read, such as one past a 64-bit integer or `1e400`.

use std::borrow::Cow;

use simd_json::StaticNode;
use simd_json::tape::Node as TapeNode;

use super::node::Node;
use super::read::{Given, Json, Property, Reader};
use super::schema::Schema;
use crate::js::{self, stack};
use crate::json::{self, EMPTY, Key, Map, Value};
use crate::{Error, Result};

/// A value on the tape: its nodes, its header first.
#[derive(Clone, Copy)]
struct Tape<'t, 'i>(&'t [TapeNode<'i>]);

/// How many nodes the value a node heads takes up, itself among them.
fn size(node: &TapeNode) -> usize {
    match node {
        TapeNode::Object { count, .. } | TapeNode::Array { count, .. } => count + 1,
        _ => 1,
    }
}

/// An array's items, or an object's entries, from the nodes after the header.
struct Items<'t, 'i> {
    rest: &'t [TapeNode<'i>],
    left: usize,
}

impl<'t, 'i> Items<'t, 'i> {
    fn value(&mut self) -> Tape<'t, 'i> {
        let (value, rest) = self.rest.split_at(size(&self.rest[0]));
        self.rest = rest;
        Tape(value)
    }
}

impl<'t, 'i> Iterator for Items<'t, 'i> {
    type Item = Tape<'t, 'i>;

    fn next(&mut self) -> Option<Self::Item> {
        self.left = self.left.checked_sub(1)?;
        Some(self.value())
    }
}

/// An object's keys and values.
struct Entries<'t, 'i>(Items<'t, 'i>);

impl<'t, 'i> Iterator for Entries<'t, 'i> {
    type Item = (&'i str, Tape<'t, 'i>);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.left = self.0.left.checked_sub(1)?;
        let TapeNode::String(key) = self.0.rest[0] else {
            unreachable!("an object's key is a string")
        };
        self.0.rest = &self.0.rest[1..];
        Some((key, self.0.value()))
    }
}

impl<'t, 'i> Tape<'t, 'i> {
    fn head(self) -> &'t TapeNode<'i> {
        &self.0[0]
    }

    fn entries(self) -> Option<Entries<'t, 'i>> {
        match *self.head() {
            TapeNode::Object { len, .. } => Some(Entries(Items {
                rest: &self.0[1..],
                left: len,
            })),
            _ => None,
        }
    }

    fn array(self) -> Option<Items<'t, 'i>> {
        match *self.head() {
            TapeNode::Array { len, .. } => Some(Items {
                rest: &self.0[1..],
                left: len,
            }),
            _ => None,
        }
    }

    fn str(self) -> Option<&'i str> {
        match *self.head() {
            TapeNode::String(text) => Some(text),
            _ => None,
        }
    }

    /// The value as `JSON.parse` gives it.
    fn value(self) -> Value {
        match self.head() {
            TapeNode::String(text) => Value::String((*text).into()),
            TapeNode::Static(StaticNode::Null) => Value::Null,
            TapeNode::Static(StaticNode::Bool(boolean)) => Value::Bool(*boolean),
            TapeNode::Static(number) => js::number(as_f64(number)),
            TapeNode::Array { .. } => Value::Array(
                self.array()
                    .into_iter()
                    .flatten()
                    .map(|item| stack::grow(|| item.value()))
                    .collect(),
            ),
            TapeNode::Object { .. } => {
                let mut object = Map::new();
                for (key, item) in self.entries().into_iter().flatten() {
                    object.insert(key.into(), stack::grow(|| item.value()));
                }
                Value::Object(object.into_js_order())
            }
        }
    }
}

/// A number on the tape as a double, as JavaScript holds every number.
fn as_f64(number: &StaticNode) -> f64 {
    match *number {
        StaticNode::I64(number) => number as f64,
        StaticNode::U64(number) => number as f64,
        StaticNode::F64(number) => number,
        StaticNode::Null | StaticNode::Bool(_) => unreachable!("not a number"),
    }
}

impl<'t, 'i: 't> Json<'t> for Tape<'t, 'i> {
    fn truthy(self) -> bool {
        match self.head() {
            TapeNode::String(text) => !text.is_empty(),
            TapeNode::Static(StaticNode::Null) => false,
            TapeNode::Static(StaticNode::Bool(boolean)) => *boolean,
            TapeNode::Static(number) => js::number_to_boolean(as_f64(number)),
            TapeNode::Array { .. } | TapeNode::Object { .. } => true,
        }
    }

    fn fields<const N: usize>(self, keys: [&'static str; N]) -> [Option<Self>; N] {
        let mut fields = [None; N];
        // A repeated key's last value is the one `JSON.parse` keeps.
        for (name, value) in self.entries().into_iter().flatten() {
            if let Some(index) = keys.iter().position(|&key| key == name) {
                fields[index] = Some(value);
            }
        }
        fields
    }

    fn string(self) -> Result<Cow<'t, str>> {
        match self.str() {
            Some(text) => Ok(Cow::Borrowed(text)),
            None => Ok(Cow::Owned(js::string_of(&self.value())?.into_owned())),
        }
    }

    fn text(self) -> Option<Cow<'t, str>> {
        self.str().map(Cow::Borrowed)
    }

    fn items(self) -> Option<impl Iterator<Item = Self>> {
        self.array()
    }

    fn attrs(self, names: &[Key]) -> Given<'t> {
        let Some(entries) = self.entries() else {
            return match self.truthy() {
                true => Given::Object(Cow::Borrowed(&EMPTY)),
                false => Given::Falsy(self.value()),
            };
        };
        let mut given: Vec<Option<Property<'t>>> = (0..names.len()).map(|_| None).collect();
        for (name, value) in entries {
            if let Some(index) = names.iter().position(|key| key.as_str() == name) {
                given[index] = Some(match value.str() {
                    Some(text) => Property::Text(text),
                    None => Property::Value(value.value()),
                });
            }
        }
        Given::Named(given)
    }
}

/// Whether the text escapes a UTF-16 surrogate, which the tape can't hold unpaired: `\uD800`
/// to `\uDFFF`, or text that only looks like one, an escaped backslash before `uD800`.
fn escapes_surrogate(text: &str) -> bool {
    let bytes = text.as_bytes();
    memchr::memchr_iter(b'\\', bytes).any(|at| {
        matches!(
            bytes.get(at + 1..at + 4),
            Some([b'u', b'd' | b'D', b'8' | b'9' | b'a'..=b'f' | b'A'..=b'F'])
        )
    })
}

/// `JSON.parse(text)` failed.
fn syntax_error() -> Error {
    Error::Syntax("Invalid JSON".into())
}

impl Node<'static> {
    /// `Node.fromJSON(schema, JSON.parse(text))`.
    pub fn from_json_text(schema: &Schema, text: &str) -> Result<Node<'static>> {
        let slow = || {
            let json = json::from_str(text).map_err(|_| syntax_error())?;
            Node::from_json(schema, &json)
        };
        // A byte order mark is no JSON to `JSON.parse`; the slow path says so.
        if escapes_surrogate(text) || text.starts_with('\u{FEFF}') {
            return slow();
        }
        let mut bytes = text.as_bytes().to_vec();
        let Ok(tape) = simd_json::to_tape(&mut bytes) else {
            return slow();
        };
        let mut reader = Reader::new(schema);
        let id = reader.node(Tape(&tape.0))?;
        Ok(Node::at(reader.finish(), id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_escaped_surrogates() {
        assert!(escapes_surrogate(r#""\ud800""#));
        assert!(escapes_surrogate(r#""\uDBFF\uDC00""#));
        assert!(!escapes_surrogate(r#""\u00e9 \uE000""#));
        assert!(!escapes_surrogate("\"\\u"));
    }

    #[test]
    fn reads_values_as_json_parse_does() {
        let cases = [
            r#"{"1":1,"b":2,"0":3}"#,
            r#"{"a":1,"a":2}"#,
            "[-0, 1.0, 9007199254740993, 5e-324, 1e-400, 18446744073709551615]",
            r#"["", "\u00e9", "é", "\u0000", "\/"]"#,
        ];
        for case in cases {
            let mut bytes = case.as_bytes().to_vec();
            let tape = simd_json::to_tape(&mut bytes).unwrap();
            let expected = json::from_str(case).unwrap();
            assert_eq!(
                json::stringify(&Tape(&tape.0).value()),
                json::stringify(&expected),
                "{case}"
            );
        }
    }
}
