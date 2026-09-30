//! JSON values as a chunk holds them: attributes, read in place.

use std::borrow::Cow;
use std::fmt;

use super::{Chunk, Holder, corrupt};
use crate::js::TypeOf;
use crate::js::json::{write_number, write_string};
use crate::js::stack;
use crate::js::value::{Nullish, cannot_read};
use crate::json::{EMPTY, Key, Map, Number, Value};
use crate::model::compare_deep::entries_equal;

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
    /// The value a record with this tag holds in its words `a` and `b`, `string` reading a
    /// string's text.
    pub fn kind<'v>(self, a: u32, b: u32, string: impl FnOnce(u32, u32) -> &'v str) -> Kind<'v> {
        let bits = u64::from(a) | u64::from(b) << 32;
        match self {
            Tag::Null => Kind::Null,
            Tag::False => Kind::Bool(false),
            Tag::True => Kind::Bool(true),
            Tag::Int => Kind::Number(Number::from(bits as i64)),
            Tag::UInt => Kind::Number(Number::from(bits)),
            Tag::Float => {
                Kind::Number(Number::from_f64(f64::from_bits(bits)).unwrap_or_else(|| corrupt()))
            }
            Tag::String => Kind::String(string(a, b)),
            Tag::Array => Kind::Array(b),
            Tag::Object => Kind::Object(b),
        }
    }

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
    pub(crate) fn at(chunk: &'c Chunk<'c>, reference: u32) -> ValueRef<'c> {
        let (chunk, index) = chunk.resolve(reference);
        ValueRef { chunk, index }
    }

    pub fn kind(self) -> Kind<'c> {
        let (tag, a, b) = self.chunk.value(self.index);
        Tag::from_word(tag).kind(a, b, |start, len| self.chunk.string(start, len))
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
        self.id() == other.id()
    }

    /// An identity for the value: its chunk's, and its index there.
    pub fn id(self) -> (usize, u32) {
        (self.chunk.bytes().as_ptr() as usize, self.index)
    }

    /// The value's tag, read without its contents.
    fn tag(self) -> Tag {
        Tag::from_word(self.chunk.value(self.index).0)
    }

    pub fn is_null(self) -> bool {
        self.tag() == Tag::Null
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

    /// `String(value)`.
    pub fn to_js_string(self) -> crate::Result<Cow<'c, str>> {
        JsonView::to_js_string(self)
    }

    /// `String(value)`, `None` being `undefined`.
    pub fn string(value: Option<ValueRef<'c>>) -> crate::Result<Cow<'c, str>> {
        match value {
            Some(value) => value.to_js_string(),
            None => Ok(Cow::Borrowed(Nullish::Undefined.name())),
        }
    }

    pub fn truthy(self) -> bool {
        JsonView::truthy(self)
    }

    /// `value ?? otherwise`, `None` being `undefined`.
    pub fn coalesce(value: impl Into<Option<ValueRef<'c>>>, otherwise: ValueRef<'c>) -> Self {
        value
            .into()
            .filter(|value| !value.is_null())
            .unwrap_or(otherwise)
    }

    /// `value.key`: the own property, as no value inherits one of the names read here, or the
    /// `TypeError` of reading a property of `null`.
    pub fn read(self, key: &str) -> crate::Result<Option<ValueRef<'c>>> {
        debug_assert!(key != "length" && key.parse::<usize>().is_err());
        match self.is_null() {
            true => Err(cannot_read(Nullish::Null, key)),
            false => Ok(self.get(key)),
        }
    }

    /// `Number(value)`.
    pub fn to_number(self) -> crate::Result<f64> {
        Ok(match self.kind() {
            Kind::Null => 0.0,
            Kind::Bool(boolean) => f64::from(u8::from(boolean)),
            Kind::Number(number) => number.as_f64().unwrap_or(f64::NAN),
            Kind::String(string) => crate::js::string_to_number(string),
            Kind::Array(_) | Kind::Object(_) => crate::js::string_to_number(&self.to_js_string()?),
        })
    }

    pub fn is_object(self) -> bool {
        self.tag() == Tag::Object
    }

    pub fn is_array(self) -> bool {
        self.tag() == Tag::Array
    }

    /// An array's or object's number of items, and 0 for anything else.
    pub fn len(self) -> usize {
        let (tag, _, len) = self.chunk.value(self.index);
        match Tag::from_word(tag) {
            Tag::Array | Tag::Object => len as usize,
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
        let (start, len) = self.span(Tag::Object);
        let index = self
            .chunk
            .find_entry(self.index, start, len, key.as_bytes())?;
        Some(ValueRef {
            chunk: self.chunk,
            index,
        })
    }

    pub fn to_value(self) -> Value {
        JsonView::to_value(self).into_owned()
    }

    /// An object's entries as a map; empty for anything else.
    pub fn to_map(self) -> Map {
        to_map(self)
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

    /// `compareDeep` of an object with `map`.
    pub fn equals_map(self, map: &Map) -> bool {
        self.is_object() && entries_equal(self.entries(), map.len(), |key| map.get(key))
    }
}

/// A JSON value however it's held, as `compareDeep`, an attribute's check and a builder read
/// it: a [`Value`], a value in a chunk, or one a builder has written.
pub(crate) trait JsonView<'v>: Copy {
    fn kind(self) -> Kind<'v>;

    /// An array's items; none for anything else.
    fn items(self) -> impl ExactSizeIterator<Item = Self>;

    /// An object's keys and values, in order; none for anything else.
    fn entries(self) -> impl ExactSizeIterator<Item = (&'v str, Self)>;

    /// An object's value for `key`.
    fn get(self, key: &str) -> Option<Self> {
        self.entries()
            .find(|&(name, _)| name == key)
            .map(|(_, value)| value)
    }

    /// Where the value is held, for a view that knows: values held in one place are equal
    /// without being read.
    fn place(self) -> Option<(usize, u32)> {
        None
    }

    fn type_of(self) -> TypeOf {
        match self.kind() {
            Kind::Null => TypeOf::Null,
            Kind::Bool(_) => TypeOf::Boolean,
            Kind::Number(_) => TypeOf::Number,
            Kind::String(_) => TypeOf::String,
            Kind::Array(_) | Kind::Object(_) => TypeOf::Object,
        }
    }

    /// `String(value)`, as [`js::to_string`](crate::js::to_string) gives it.
    fn to_js_string(self) -> crate::Result<Cow<'v, str>> {
        Ok(match self.kind() {
            Kind::Null => Cow::Borrowed("null"),
            Kind::Bool(true) => Cow::Borrowed("true"),
            Kind::Bool(false) => Cow::Borrowed("false"),
            Kind::Number(number) => Cow::Owned(crate::js::number_to_string(
                number.as_f64().unwrap_or(f64::NAN),
            )),
            Kind::String(string) => Cow::Borrowed(string),
            Kind::Array(_) => {
                let mut parts = Vec::new();
                for item in self.items() {
                    parts.push(match item.kind() {
                        Kind::Null => Cow::Borrowed(""),
                        _ => stack::grow(|| item.to_js_string())?,
                    });
                }
                Cow::Owned(parts.join(","))
            }
            Kind::Object(_) if self.get("toString").is_some() => {
                return Err(crate::js::no_primitive());
            }
            Kind::Object(_) => Cow::Borrowed("[object Object]"),
        })
    }

    fn truthy(self) -> bool {
        match self.kind() {
            Kind::Null | Kind::Bool(false) => false,
            Kind::Number(number) => number.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
            Kind::String(string) => !string.is_empty(),
            Kind::Bool(true) | Kind::Array(_) | Kind::Object(_) => true,
        }
    }

    fn to_value(self) -> Cow<'v, Value> {
        Cow::Owned(match self.kind() {
            Kind::Null => Value::Null,
            Kind::Bool(boolean) => Value::Bool(boolean),
            Kind::Number(number) => Value::Number(number),
            Kind::String(string) => Value::String(string.into()),
            Kind::Array(_) => Value::Array(
                self.items()
                    .map(|item| stack::grow(|| item.to_value().into_owned()))
                    .collect(),
            ),
            Kind::Object(_) => Value::Object(to_map(self)),
        })
    }
}

fn to_map<'v>(object: impl JsonView<'v>) -> Map {
    object
        .entries()
        .map(|(key, value)| {
            (
                Key::from(key),
                stack::grow(|| value.to_value().into_owned()),
            )
        })
        .collect()
}

impl<'c> JsonView<'c> for ValueRef<'c> {
    #[inline]
    fn kind(self) -> Kind<'c> {
        ValueRef::kind(self)
    }

    fn items(self) -> impl ExactSizeIterator<Item = Self> {
        ValueRef::items(self)
    }

    fn entries(self) -> impl ExactSizeIterator<Item = (&'c str, Self)> {
        ValueRef::entries(self)
    }

    fn get(self, key: &str) -> Option<Self> {
        ValueRef::get(self, key)
    }

    #[inline]
    fn place(self) -> Option<(usize, u32)> {
        Some(self.id())
    }

    /// The tag's type, without reading a string or a number.
    #[inline]
    fn type_of(self) -> TypeOf {
        match Tag::from_word(self.chunk.value(self.index).0) {
            Tag::Null => TypeOf::Null,
            Tag::False | Tag::True => TypeOf::Boolean,
            Tag::Int | Tag::UInt | Tag::Float => TypeOf::Number,
            Tag::String => TypeOf::String,
            Tag::Array | Tag::Object => TypeOf::Object,
        }
    }
}

impl<'v> JsonView<'v> for &'v Value {
    #[inline]
    fn kind(self) -> Kind<'v> {
        match self {
            Value::Null => Kind::Null,
            Value::Bool(boolean) => Kind::Bool(*boolean),
            Value::Number(number) => Kind::Number(number.clone()),
            Value::String(string) => Kind::String(string),
            Value::Array(items) => Kind::Array(items.len() as u32),
            Value::Object(map) => Kind::Object(map.len() as u32),
        }
    }

    fn items(self) -> impl ExactSizeIterator<Item = Self> {
        self.as_array().map_or(&[][..], Vec::as_slice).iter()
    }

    fn entries(self) -> impl ExactSizeIterator<Item = (&'v str, Self)> {
        self.as_object()
            .unwrap_or(&EMPTY)
            .iter()
            .map(|(key, value)| (key.as_str(), value))
    }

    fn to_value(self) -> Cow<'v, Value> {
        Cow::Borrowed(self)
    }
}

impl fmt::Debug for ValueRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut out = String::new();
        self.write_json(&mut out);
        f.write_str(&out)
    }
}

#[cfg(test)]
mod tests {
    use super::ValueRef;
    use crate::dom::DomSpec;
    use crate::{Node, api, json};

    /// `attrs.object.b`, `attrs.a ?? attrs.b`, `${attrs.a}` and `attrs.a ?? ""` as a spec's
    /// child, as JavaScript reads them.
    #[test]
    fn reads_as_javascript_does() {
        let spec = r#"{"nodes": {"doc": {"attrs": {"object": {}, "null": {}, "text": {}}},
                                 "text": {}}}"#;
        let schema = api::schema(&json::from_str(spec).expect("JSON")).expect("a schema");
        let doc = r#"{"type": "doc", "attrs": {"object": {"b": 1}, "null": null, "text": "t"}}"#;
        let doc = Node::from_json(&schema, &json::from_str(doc).expect("JSON")).expect("a doc");
        let attrs = doc.attrs_view();
        let [object, null, text] =
            ["object", "null", "text"].map(|name| attrs.get(name).expect("an attribute"));

        assert_eq!(
            object.read("b").unwrap().and_then(ValueRef::as_f64),
            Some(1.0)
        );
        assert!(object.read("c").unwrap().is_none());
        assert!(text.read("b").unwrap().is_none());
        assert_eq!(
            null.read("b").unwrap_err().to_string(),
            "TypeError: Cannot read properties of null (reading 'b')"
        );

        assert!(ValueRef::coalesce(null, text).ptr_eq(text));
        assert!(ValueRef::coalesce(None::<ValueRef>, text).ptr_eq(text));
        assert!(ValueRef::coalesce(object, text).ptr_eq(object));

        let strings = [None, Some(null), Some(text)].map(|value| ValueRef::string(value).unwrap());
        assert_eq!(strings, ["undefined", "null", "t"]);

        let child = |value| match DomSpec::<()>::attr_or(value, "fallback") {
            DomSpec::Attr(value) => value.to_js_string().unwrap().into_owned(),
            DomSpec::Text(text) => format!("text {text}"),
            _ => unreachable!("an attribute or text"),
        };
        assert_eq!(child(Some(text)), "t");
        assert_eq!(child(Some(null)), "text fallback");
        assert_eq!(child(None), "text fallback");
    }
}
