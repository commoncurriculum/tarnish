//! Parsing JSON text as `JSON.parse` does, to any depth: the parser keeps its place in a vector,
//! and so does the value being built.

use std::collections::HashMap;
use std::fmt;

use json_event_parser::{JsonEvent, LowLevelJsonParser, LowLevelJsonParserResult};
use serde::de::{DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};

use super::{Key, Map, Value};
use crate::{number, stack};

/// The text isn't JSON, so `JSON.parse` throws.
#[derive(Debug)]
pub struct SyntaxError;

/// Past this many keys, parsing an object finds repeated keys through a hash table.
const LINEAR_KEYS: usize = 32;

/// The stack serde_json takes for JSON nested as deep as it reads, 128 levels, with room to
/// spare: 200 KiB unoptimized and 45 KiB optimized, more than a level of a recursion that calls
/// this is sure of.
const SERDE_ROOM: usize = if cfg!(debug_assertions) {
    512 << 10
} else {
    128 << 10
};

/// `JSON.parse(text)`, as the value JavaScript holds: every number is a double, so an integral
/// one is an integer and the rest are floats; a repeated key keeps its first place and its last
/// value; and keys that are array indices come first, in ascending order. A lone surrogate,
/// which a Rust string can't hold, is U+FFFD.
// bounded: once, on the text with its lone surrogates replaced
pub fn from_str(text: &str) -> Result<Value, SyntaxError> {
    // The parser skips a byte order mark, which `JSON.parse` doesn't take.
    if text.starts_with('\u{FEFF}') {
        return Err(SyntaxError);
    }
    // serde_json reads JSON far faster, and takes no text `JSON.parse` refuses. Text that ends
    // early is no JSON to either; what else serde_json refuses, nesting past its limit or a
    // number past a double, the event parser decides. Both refuse a lone surrogate.
    match stack::with_room(SERDE_ROOM, || read_with_serde(text)) {
        Ok(value) => Ok(value),
        Err(error) if error.is_eof() => Err(SyntaxError),
        Err(_) => read_events(text).or_else(|error| match lone_surrogates_replaced(text) {
            Some(text) => from_str(&text),
            None => Err(error),
        }),
    }
}

/// The text with each escape of a lone surrogate made an escape of U+FFFD, or `None` when it has
/// none.
fn lone_surrogates_replaced(text: &str) -> Option<String> {
    let unit_at = |at: usize| match text.get(at..at + 2) {
        Some("\\u") => u16::from_str_radix(text.get(at + 2..at + 6)?, 16).ok(),
        _ => None,
    };
    let surrogate = |unit: u16| (0xD800..0xE000).contains(&unit);
    let high = |unit: u16| (0xD800..0xDC00).contains(&unit);
    let mut replaced = String::new();
    let (mut copied, mut at) = (0, 0);
    while let Some(found) = text[at..].find('\\') {
        at += found;
        match unit_at(at) {
            Some(unit)
                if high(unit)
                    && unit_at(at + 6).is_some_and(|next| surrogate(next) && !high(next)) =>
            {
                at += 12
            }
            Some(unit) if surrogate(unit) => {
                replaced.push_str(&text[copied..at]);
                replaced.push_str("\\ufffd");
                at += 6;
                copied = at;
            }
            Some(_) => at += 6,
            None => at += 1 + text[at + 1..].chars().next().map_or(0, char::len_utf8),
        }
    }
    (copied > 0).then(|| replaced + &text[copied..])
}

fn read_with_serde(text: &str) -> Result<Value, serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let value = Seed.deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(value)
}

fn read_events(text: &str) -> Result<Value, SyntaxError> {
    let mut parser = LowLevelJsonParser::new().with_max_stack_size(usize::MAX);
    let mut input = text.as_bytes();
    let mut builder = Builder::default();
    loop {
        let LowLevelJsonParserResult {
            event,
            consumed_bytes,
        } = parser.parse_next(input, true);
        input = &input[consumed_bytes..];
        match event {
            None => {}
            Some(Err(_)) => return Err(SyntaxError),
            Some(Ok(JsonEvent::Eof)) => return builder.root.ok_or(SyntaxError),
            Some(Ok(event)) => builder.add(event),
        }
    }
}

/// A value serde_json reads.
struct Seed;

impl<'de> DeserializeSeed<'de> for Seed {
    type Value = Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(Seed)
    }
}

impl<'de> Visitor<'de> for Seed {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("JSON")
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_bool<E>(self, boolean: bool) -> Result<Value, E> {
        Ok(Value::Bool(boolean))
    }

    fn visit_i64<E>(self, integer: i64) -> Result<Value, E> {
        Ok(number(integer as f64))
    }

    fn visit_u64<E>(self, integer: u64) -> Result<Value, E> {
        Ok(number(integer as f64))
    }

    fn visit_f64<E>(self, float: f64) -> Result<Value, E> {
        Ok(number(float))
    }

    fn visit_str<E>(self, text: &str) -> Result<Value, E> {
        Ok(Value::String(text.to_owned()))
    }

    fn visit_string<E>(self, text: String) -> Result<Value, E> {
        Ok(Value::String(text))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = items.next_element_seed(Seed)? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut entries: A) -> Result<Value, A::Error> {
        let mut object = Object::default();
        while let Some(key) = entries.next_key_seed(KeySeed)? {
            let value = entries.next_value_seed(Seed)?;
            object.insert(key, value);
        }
        Ok(Value::Object(object.finish()))
    }
}

/// A key serde_json reads.
struct KeySeed;

impl<'de> DeserializeSeed<'de> for KeySeed {
    type Value = Key;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Key, D::Error> {
        deserializer.deserialize_str(KeySeed)
    }
}

impl<'de> Visitor<'de> for KeySeed {
    type Value = Key;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a key")
    }

    fn visit_str<E>(self, text: &str) -> Result<Key, E> {
        Ok(Key::from(text))
    }
}

#[derive(Default)]
struct Builder {
    open: Vec<Open>,
    root: Option<Value>,
}

enum Open {
    Array(Vec<Value>),
    Object(Object, Key),
}

impl Builder {
    fn add(&mut self, event: JsonEvent) {
        let value = match event {
            JsonEvent::StartArray => return self.open.push(Open::Array(Vec::new())),
            JsonEvent::StartObject => {
                return self
                    .open
                    .push(Open::Object(Object::default(), Key::default()));
            }
            JsonEvent::ObjectKey(name) => {
                if let Some(Open::Object(_, key)) = self.open.last_mut() {
                    *key = Key::from(name.as_ref());
                }
                return;
            }
            JsonEvent::EndArray | JsonEvent::EndObject => match self.open.pop() {
                Some(Open::Array(items)) => Value::Array(items),
                Some(Open::Object(object, _)) => Value::Object(object.finish()),
                None => unreachable!("the parser closes only what it opened"),
            },
            JsonEvent::String(text) => Value::String(text.into_owned()),
            JsonEvent::Number(text) => {
                number(text.parse().expect("the parser checks a number's syntax"))
            }
            JsonEvent::Boolean(boolean) => Value::Bool(boolean),
            JsonEvent::Null => Value::Null,
            JsonEvent::Eof => unreachable!("the end is handled by the caller"),
        };
        match self.open.last_mut() {
            None => self.root = Some(value),
            Some(Open::Array(items)) => items.push(value),
            Some(Open::Object(object, key)) => object.insert(std::mem::take(key), value),
        }
    }
}

/// An object being read: a repeated key keeps its first place and takes its last value.
#[derive(Default)]
struct Object {
    map: Map,
    index: Option<HashMap<Key, usize>>,
}

impl Object {
    fn insert(&mut self, key: Key, value: Value) {
        if self.index.is_none() && self.map.len() >= LINEAR_KEYS {
            self.index = Some(
                self.map
                    .keys()
                    .enumerate()
                    .map(|(position, key)| (key.clone(), position))
                    .collect(),
            );
        }
        match &mut self.index {
            None => {
                self.map.insert(key, value);
            }
            Some(index) => match index.get(&key) {
                Some(&position) => self.map.entries[position].1 = value,
                None => {
                    index.insert(key.clone(), self.map.len());
                    self.map.push(key, value);
                }
            },
        }
    }

    fn finish(self) -> Map {
        self.map.into_js_order()
    }
}

#[cfg(test)]
mod tests {
    use super::from_str;
    use crate::json::stringify;

    /// Whatever serde_json reads, the event parser reads the same, as `JSON.stringify` writes
    /// it.
    #[test]
    fn serde_reads_as_the_event_parser_does() {
        let mut read = 0;
        for text in crate::random::json_texts().take(200_000) {
            if let Ok(fast) = super::read_with_serde(&text) {
                let slow = super::read_events(&text).expect("the event parser reads it too");
                assert!(stringify(&fast) == stringify(&slow), "{text}");
                read += 1;
            }
        }
        assert!(read > 100_000, "serde_json read {read}");
    }

    #[test]
    fn reads_a_lone_surrogate_as_u_fffd() {
        for (text, expected) in [
            (r#""\ud83d""#, "\u{FFFD}"),
            (r#""a\udc00b""#, "a\u{FFFD}b"),
            (r#""\ud83d😀""#, "\u{FFFD}😀"),
            (r#""\ude00\ud83d""#, "\u{FFFD}\u{FFFD}"),
            (r#""é\\\ud800\\ud800""#, "é\\\u{FFFD}\\ud800"),
        ] {
            assert_eq!(from_str(text).expect(text), expected, "{text}");
        }
        let object = from_str(r#"{"\udfff":[1]}"#).expect("an object");
        assert_eq!(stringify(&object), "{\"\u{FFFD}\":[1]}");
        for text in [r#""\ud800"#, r#"\ud800"#, "\"\\\u{e9}\\ud800\""] {
            assert!(from_str(text).is_err(), "{text}");
        }
    }

    #[test]
    fn parses_any_depth() {
        let depth = 1_000_000;
        let text = "[".repeat(depth) + &"]".repeat(depth);
        let value = from_str(&text).unwrap();
        let opened = crate::json::events(&value)
            .filter(|event| matches!(event, crate::json::Event::Open(_)))
            .count();
        assert_eq!(opened, depth);
        assert_eq!(value.clone(), value);
    }
}
