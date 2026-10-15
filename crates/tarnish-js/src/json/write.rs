//! `JSON.stringify`, as serde_json writes JSON: it escapes what JavaScript escapes, with
//! lowercase hex, and writes a number that isn't finite as `null`. ryu-js writes each number as
//! JavaScript's `Number.prototype.toString` does, `1` for `1.0` and `100000000000000000000` for
//! `1e20`, where serde_json's own formatter would write `1.0` and `1e+20`.

use std::fmt;
use std::io;

use serde::Serialize;
use serde::ser::{Error as _, SerializeMap, Serializer};
use serde_json::ser::{CompactFormatter, Formatter, PrettyFormatter};
use serde_json::value::RawValue;

use super::{Map, Value};
use crate::stack;

/// serde_json's formatter, its numbers written as JavaScript writes them.
pub struct JsFormatter<F = CompactFormatter>(F);

impl JsFormatter {
    /// `JSON.stringify(value)`'s layout, with nothing between the parts.
    pub fn compact() -> Self {
        JsFormatter(CompactFormatter)
    }
}

impl JsFormatter<PrettyFormatter<'static>> {
    /// `JSON.stringify(value, null, 2)`'s layout.
    pub fn pretty() -> Self {
        JsFormatter(PrettyFormatter::with_indent(b"  "))
    }
}

impl<F: Formatter> Formatter for JsFormatter<F> {
    fn write_f64<W: ?Sized + io::Write>(&mut self, writer: &mut W, value: f64) -> io::Result<()> {
        // serde_json writes a double that isn't finite as `null` before it gets here.
        writer.write_all(ryu_js::Buffer::new().format_finite(value).as_bytes())
    }

    fn begin_array<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.0.begin_array(writer)
    }

    fn end_array<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.0.end_array(writer)
    }

    fn begin_array_value<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        self.0.begin_array_value(writer, first)
    }

    fn end_array_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.0.end_array_value(writer)
    }

    fn begin_object<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.0.begin_object(writer)
    }

    fn end_object<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.0.end_object(writer)
    }

    fn begin_object_key<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        self.0.begin_object_key(writer, first)
    }

    fn begin_object_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.0.begin_object_value(writer)
    }

    fn end_object_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.0.end_object_value(writer)
    }
}

/// `JSON.stringify` of what `value` serializes as, written onto `out`.
pub fn write<T: Serialize + ?Sized>(out: &mut Vec<u8>, value: &T) {
    write_with(out, value, JsFormatter::compact());
}

fn write_with<T: Serialize + ?Sized, F: Formatter>(out: &mut Vec<u8>, value: &T, formatter: F) {
    let mut serializer = serde_json::Serializer::with_formatter(out, formatter);
    value
        .serialize(&mut serializer)
        .expect("a value tarnish serializes writes to memory without failing");
}

/// The text written, which is JSON, as a string.
fn utf8(out: Vec<u8>) -> String {
    String::from_utf8(out).expect("serde_json writes UTF-8")
}

/// `JSON.stringify` of what `value` serializes as, in a string with room for `capacity` bytes.
pub fn to_string_with_capacity<T: Serialize + ?Sized>(value: &T, capacity: usize) -> String {
    let mut out = Vec::with_capacity(capacity);
    write(&mut out, value);
    utf8(out)
}

/// `JSON.stringify` of what `value` serializes as.
pub fn to_string<T: Serialize + ?Sized>(value: &T) -> String {
    to_string_with_capacity(value, 0)
}

/// `JSON.stringify(value)`, however deeply it nests.
pub fn stringify(value: &Value) -> String {
    to_string(value)
}

/// `JSON.stringify(value, null, 2)`, however deeply it nests.
pub fn stringify_pretty(value: &Value) -> String {
    let mut out = Vec::new();
    write_with(&mut out, value, JsFormatter::pretty());
    utf8(out)
}

/// `JSON.stringify` of an object holding `entries`.
pub fn stringify_entries<'k, 'v>(entries: impl Iterator<Item = (&'k str, &'v Value)>) -> String {
    // Most attribute trailers fit, where growing from one byte would reallocate several times.
    let mut out = Vec::with_capacity(64);
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, JsFormatter::compact());
    let written = serializer.collect_map(entries);
    written.expect("a value tarnish serializes writes to memory without failing");
    utf8(out)
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Null => serializer.serialize_unit(),
            Value::Bool(boolean) => serializer.serialize_bool(*boolean),
            Value::Number(number) => serializer.serialize_f64(number.as_f64()),
            Value::String(string) => serializer.serialize_str(string),
            // One check of the stack for the level, rather than one for each item.
            Value::Array(items) => stack::grow(|| serializer.collect_seq(items)),
            Value::Object(entries) => stack::grow(|| entries.serialize(serializer)),
        }
    }
}

impl Serialize for Map {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.len()))?;
        for (key, value) in self.iter() {
            map.serialize_entry(key.as_str(), value)?;
        }
        map.end()
    }
}

/// A string held as UTF-16 units, which may hold a lone surrogate, as `JSON.stringify` writes
/// it: a lone surrogate as its escape, which no Rust string can hold.
pub struct Units<'a>(pub &'a [u16]);

impl Serialize for Units<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if let Ok(string) = String::from_utf16(self.0) {
            return serializer.serialize_str(&string);
        }
        let mut written = String::with_capacity(self.0.len() + 2);
        written.push('"');
        for decoded in char::decode_utf16(self.0.iter().copied()) {
            let mut buffer = [0; 4];
            match decoded {
                Ok(character) => {
                    // serde_json escapes the character as it escapes it in any string.
                    let escaped = serde_json::to_string(character.encode_utf8(&mut buffer))
                        .map_err(S::Error::custom)?;
                    written.push_str(&escaped[1..escaped.len() - 1]);
                }
                Err(lone) => written.push_str(&format!("\\u{:04x}", lone.unpaired_surrogate())),
            }
        }
        written.push('"');
        RawValue::from_string(written)
            .map_err(S::Error::custom)?
            .serialize(serializer)
    }
}

/// A value displays as `JSON.stringify` writes it.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&stringify(self))
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&stringify(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::from_str;

    /// Numbers as JavaScript writes them, where serde_json's own formatter writes them otherwise.
    #[test]
    fn writes_numbers_as_javascript_does() {
        let written = [
            (1e20, "100000000000000000000"),
            (1e21, "1e+21"),
            (1e-6, "0.000001"),
            (1e-7, "1e-7"),
            (-0.0, "0"),
            (9007199254740992.0, "9007199254740992"),
            (0.1 + 0.2, "0.30000000000000004"),
            (f64::NAN, "null"),
            (f64::INFINITY, "null"),
        ];
        for (number, text) in written {
            assert_eq!(stringify(&crate::number(number)), text, "{number}");
        }
    }

    /// Every character JavaScript escapes, as it escapes it, and a lone surrogate as its escape.
    #[test]
    fn escapes_as_javascript_does() {
        let controls: String = (0..0x20u8).map(char::from).collect();
        assert_eq!(
            stringify(&Value::String(format!("{controls}\"\\/\u{7f}é😀").into())),
            "\"\\u0000\\u0001\\u0002\\u0003\\u0004\\u0005\\u0006\\u0007\\b\\t\\n\\u000b\\f\\r\
             \\u000e\\u000f\\u0010\\u0011\\u0012\\u0013\\u0014\\u0015\\u0016\\u0017\\u0018\
             \\u0019\\u001a\\u001b\\u001c\\u001d\\u001e\\u001f\\\"\\\\/\u{7f}é😀\""
        );
        let units = [0x61, 0xD800, 0x22, 0xD83D, 0xDE00, 0xDC00];
        assert_eq!(to_string(&Units(&units)), "\"a\\ud800\\\"😀\\udc00\"");
    }

    #[test]
    fn writes_pretty_as_javascript_does() {
        let value = from_str(r#"{"a":[1,{"b":[]},{}],"c":"d"}"#).unwrap();
        assert_eq!(
            stringify_pretty(&value),
            "{\n  \"a\": [\n    1,\n    {\n      \"b\": []\n    },\n    {}\n  ],\n  \"c\": \"d\"\n}"
        );
    }
}
