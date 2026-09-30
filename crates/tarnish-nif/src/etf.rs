//! Erlang's external term format, in which the VM writes a whole term out, or makes one, in one
//! call: terms read as the JSON values Jason encodes them as, and values written as the terms
//! Jason decodes from their JSON.

use tarnish::chunk::{Kind, ValueRef};
use tarnish::js::stack;
use tarnish::json::{Key, Map, Number, Value};
use tarnish::{Field, Fields};

use tarnish::js::{MAX_SAFE_INTEGER, number_to_string};

const VERSION: u8 = 131;
const NEW_FLOAT: u8 = 70;
const SMALL_INTEGER: u8 = 97;
const INTEGER: u8 = 98;
const ATOM: u8 = 100;
const SMALL_TUPLE: u8 = 104;
const NIL: u8 = 106;
const STRING: u8 = 107;
const LIST: u8 = 108;
const BINARY: u8 = 109;
const SMALL_BIG: u8 = 110;
const LARGE_BIG: u8 = 111;
const SMALL_ATOM: u8 = 115;
const MAP: u8 = 116;
const ATOM_UTF8: u8 = 118;
const SMALL_ATOM_UTF8: u8 = 119;

/// A term Jason refuses to encode.
#[derive(Debug)]
pub struct NotJson;

/// The value of the term `term_to_binary` wrote: maps with string, atom or integer keys,
/// `Jason.OrderedObject`s, lists, strings, atoms and numbers, as Jason encodes them.
pub fn read(bytes: &[u8]) -> Result<Value, NotJson> {
    let mut reader = Reader { bytes, at: 0 };
    if reader.byte()? != VERSION {
        return Err(NotJson);
    }
    let value = reader.value(0)?;
    match reader.at == bytes.len() {
        true => Ok(value),
        false => Err(NotJson),
    }
}

/// The levels a read goes down between calls to `stack::grow`, far fewer than its red zone
/// holds.
const LEVELS: usize = 16;

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

/// A map's key.
enum ReadKey<'a> {
    /// A string, which no other key of the map can be.
    String(&'a str),
    /// An atom or integer, which may write as the same text as another of the map's keys.
    Other(Key),
    /// A struct's `__struct__` key.
    Struct,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], NotJson> {
        let taken = self.bytes.get(self.at..self.at + count).ok_or(NotJson)?;
        self.at += count;
        Ok(taken)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], NotJson> {
        Ok(*self.take(N)?.first_chunk().expect("N bytes"))
    }

    fn byte(&mut self) -> Result<u8, NotJson> {
        let byte = *self.bytes.get(self.at).ok_or(NotJson)?;
        self.at += 1;
        Ok(byte)
    }

    fn u16(&mut self) -> Result<usize, NotJson> {
        Ok(u16::from_be_bytes(self.array()?).into())
    }

    fn u32(&mut self) -> Result<usize, NotJson> {
        Ok(u32::from_be_bytes(self.array()?) as usize)
    }

    /// A binary that holds UTF-8, which is all Jason encodes.
    fn text(&mut self, length: usize) -> Result<&'a str, NotJson> {
        std::str::from_utf8(self.take(length)?).map_err(|_| NotJson)
    }

    fn value(&mut self, depth: usize) -> Result<Value, NotJson> {
        let mut value = Value::Null;
        self.value_into(depth, &mut value)?;
        Ok(value)
    }

    /// Reads a value into `slot`, which holds `null`: one read where it is kept isn't copied
    /// there from the stack.
    fn value_into(&mut self, depth: usize, slot: &mut Value) -> Result<(), NotJson> {
        *slot = match self.byte()? {
            BINARY => {
                let length = self.u32()?;
                Value::String(self.text(length)?.to_owned())
            }
            MAP => {
                let arity = self.u32()?;
                return self.below(depth, |reader, depth| reader.object(arity, depth, slot));
            }
            LIST => {
                let length = self.u32()?;
                return self.below(depth, |reader, depth| reader.list(length, depth, slot));
            }
            NIL => Value::Array(Vec::new()),
            tag @ (SMALL_INTEGER | INTEGER | SMALL_BIG | LARGE_BIG) => {
                Value::Number(self.integer(tag)?.number()?)
            }
            NEW_FLOAT => {
                let double = f64::from_be_bytes(self.array()?);
                Value::Number(Number::from_f64(double).ok_or(NotJson)?)
            }
            tag @ (ATOM | SMALL_ATOM | ATOM_UTF8 | SMALL_ATOM_UTF8) => match self.atom(tag)? {
                "nil" => Value::Null,
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                name => Value::String(name.to_owned()),
            },
            // A list of bytes.
            STRING => {
                let length = self.u16()?;
                let bytes = self.take(length)?;
                Value::Array(
                    bytes
                        .iter()
                        .map(|&byte| Value::Number(byte.into()))
                        .collect(),
                )
            }
            _ => return Err(NotJson),
        };
        Ok(())
    }

    /// Reads a level below `depth`, making room on the stack every [`LEVELS`] levels.
    fn below<T>(&mut self, depth: usize, read: impl FnOnce(&mut Self, usize) -> T) -> T {
        let depth = depth + 1;
        match depth % LEVELS {
            0 => stack::grow(|| read(self, depth)),
            _ => read(self, depth),
        }
    }

    fn list(&mut self, length: usize, depth: usize, slot: &mut Value) -> Result<(), NotJson> {
        // A term's length can't be more than its bytes, so a false one can't take up memory.
        let items = new_array(slot, length.min(self.bytes.len() - self.at));
        for _ in 0..length {
            self.value_into(depth, new_item(items))?;
        }
        // An improper list has no JSON.
        match self.byte()? {
            NIL => Ok(()),
            _ => Err(NotJson),
        }
    }

    fn object(&mut self, arity: usize, depth: usize, slot: &mut Value) -> Result<(), NotJson> {
        let object = new_object(slot, arity.min(self.bytes.len() - self.at));
        let mut unique = true;
        for index in 0..arity {
            let key = match self.key()? {
                ReadKey::String(text) => Key::from(text),
                ReadKey::Other(key) => {
                    unique = false;
                    key
                }
                // A struct's map is small, so its keys come in term order, the atom
                // `__struct__` before `values`, and before any key of its own that is a string.
                ReadKey::Struct if arity == 2 && index == 0 => {
                    *slot = Value::Object(self.ordered_object(depth)?);
                    return Ok(());
                }
                ReadKey::Struct => return Err(NotJson),
            };
            if unique {
                self.value_into(depth, new_entry(object, key))?;
            } else {
                // A key written twice keeps its first place and its last value, as parsing the
                // JSON Jason writes for it does.
                let value = self.value(depth)?;
                object.insert(key, value);
            }
        }
        in_js_order(slot);
        Ok(())
    }

    /// The rest of a `Jason.OrderedObject`: its entries, in order.
    fn ordered_object(&mut self, depth: usize) -> Result<Map, NotJson> {
        for expected in ["Elixir.Jason.OrderedObject", "values"] {
            let tag = self.byte()?;
            if !matches!(tag, ATOM | SMALL_ATOM | ATOM_UTF8 | SMALL_ATOM_UTF8)
                || self.atom(tag)? != expected
            {
                return Err(NotJson);
            }
        }
        let length = match self.byte()? {
            NIL => return Ok(Map::new()),
            LIST => self.u32()?,
            _ => return Err(NotJson),
        };
        let mut object = Map::new();
        for _ in 0..length {
            if self.take(2)? != [SMALL_TUPLE, 2] {
                return Err(NotJson);
            }
            let key = match self.key()? {
                ReadKey::String(text) => Key::from(text),
                ReadKey::Other(key) => key,
                ReadKey::Struct => return Err(NotJson),
            };
            let value = self.value(depth)?;
            object.insert(key, value);
        }
        match self.byte()? {
            NIL => Ok(object.into_js_order()),
            _ => Err(NotJson),
        }
    }

    fn key(&mut self) -> Result<ReadKey<'a>, NotJson> {
        Ok(match self.byte()? {
            BINARY => {
                let length = self.u32()?;
                ReadKey::String(self.text(length)?)
            }
            tag @ (ATOM | SMALL_ATOM | ATOM_UTF8 | SMALL_ATOM_UTF8) => match self.atom(tag)? {
                "__struct__" => ReadKey::Struct,
                name => ReadKey::Other(Key::from(name)),
            },
            tag @ (SMALL_INTEGER | INTEGER | SMALL_BIG | LARGE_BIG) => {
                ReadKey::Other(Key::from(self.integer(tag)?.text()))
            }
            _ => return Err(NotJson),
        })
    }

    fn atom(&mut self, tag: u8) -> Result<&'a str, NotJson> {
        let length = match tag {
            SMALL_ATOM | SMALL_ATOM_UTF8 => self.byte()?.into(),
            _ => self.u16()?,
        };
        let name = self.text(length)?;
        // Latin-1 atoms Jason could encode are ASCII, as every atom the VM writes this way is.
        if matches!(tag, ATOM | SMALL_ATOM) && !name.is_ascii() {
            return Err(NotJson);
        }
        Ok(name)
    }

    fn integer(&mut self, tag: u8) -> Result<Integer<'a>, NotJson> {
        Ok(match tag {
            SMALL_INTEGER => Integer::Word(self.byte()?.into()),
            INTEGER => Integer::Word(i32::from_be_bytes(self.array()?).into()),
            _ => {
                let length = match tag {
                    SMALL_BIG => self.byte()?.into(),
                    _ => self.u32()?,
                };
                let negative = self.byte()? != 0;
                Integer::Big {
                    negative,
                    digits: self.take(length)?,
                }
            }
        })
    }
}

enum Integer<'a> {
    Word(i64),
    /// A sign, and base-256 digits, lowest first.
    Big {
        negative: bool,
        digits: &'a [u8],
    },
}

impl Integer<'_> {
    /// The integer as Jason writes it and `JSON.parse` reads it back: exactly within 64 bits,
    /// as the nearest double past them.
    fn number(&self) -> Result<Number, NotJson> {
        match *self {
            Integer::Word(word) => Ok(word.into()),
            Integer::Big { negative, digits } if digits.len() <= 8 => {
                let mut bytes = [0; 8];
                bytes[..digits.len()].copy_from_slice(digits);
                let magnitude = u64::from_le_bytes(bytes);
                match negative {
                    false => Ok(magnitude.into()),
                    true if magnitude <= 1 << 63 => Ok((magnitude as i64).wrapping_neg().into()),
                    true => self.double(),
                }
            }
            Integer::Big { .. } => self.double(),
        }
    }

    fn double(&self) -> Result<Number, NotJson> {
        let double: f64 = self.text().parse().map_err(|_| NotJson)?;
        Number::from_f64(double).ok_or(NotJson)
    }

    /// The integer's decimal digits, as Jason writes an integer key.
    fn text(&self) -> String {
        let (negative, digits) = match *self {
            Integer::Word(word) => return word.to_string(),
            Integer::Big { negative, digits } => (negative, digits),
        };
        let mut limbs: Vec<u32> = digits
            .chunks(4)
            .map(|chunk| {
                let mut limb = [0; 4];
                limb[..chunk.len()].copy_from_slice(chunk);
                u32::from_le_bytes(limb)
            })
            .collect();
        // Nine decimal digits at a time, lowest first.
        let mut groups = Vec::new();
        while limbs.iter().any(|&limb| limb != 0) {
            let mut remainder = 0u64;
            for limb in limbs.iter_mut().rev() {
                let current = (remainder << 32) | u64::from(*limb);
                *limb = (current / 1_000_000_000) as u32;
                remainder = current % 1_000_000_000;
            }
            groups.push(remainder);
        }
        let mut text = String::from(if negative { "-" } else { "" });
        match groups.split_last() {
            None => text.push('0'),
            Some((highest, rest)) => {
                text.push_str(&highest.to_string());
                for group in rest.iter().rev() {
                    text.push_str(&format!("{group:09}"));
                }
            }
        }
        text
    }
}

/// `slot` made an empty array, for a reader to read items into where they are kept.
pub fn new_array(slot: &mut Value, capacity: usize) -> &mut Vec<Value> {
    *slot = Value::Array(Vec::with_capacity(capacity));
    match slot {
        Value::Array(items) => items,
        _ => unreachable!("an array was just put there"),
    }
}

/// `slot` made an empty object, for a reader to read values into where they are kept.
pub fn new_object(slot: &mut Value, capacity: usize) -> &mut Map {
    *slot = Value::Object(Map::with_capacity(capacity));
    match slot {
        Value::Object(object) => object,
        _ => unreachable!("an object was just put there"),
    }
}

/// The object in `slot` with its keys in the order `JSON.parse` gives them, as the worker reads
/// the JSON Jason writes for it.
pub fn in_js_order(slot: &mut Value) {
    if let Value::Object(object) = slot {
        *object = std::mem::take(object).into_js_order();
    }
}

/// A last item, `null` until a reader reads into it.
pub fn new_item(items: &mut Vec<Value>) -> &mut Value {
    items.push(Value::Null);
    items.last_mut().expect("an item was just pushed")
}

/// A last entry, `null` until a reader reads into it.
pub fn new_entry(object: &mut Map, key: Key) -> &mut Value {
    object.push(key, Value::Null);
    object
        .values_mut()
        .next_back()
        .expect("an entry was just pushed")
}

/// The term Jason decodes from the JSON `JSON.stringify` writes for the value.
pub fn write(value: &Value) -> Vec<u8> {
    let mut out = vec![VERSION];
    write_value(&mut out, value);
    out
}

fn write_value(out: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Null => atom(out, "nil"),
        Value::Bool(true) => atom(out, "true"),
        Value::Bool(false) => atom(out, "false"),
        Value::Number(number) => write_number(out, number),
        Value::String(text) => binary(out, text),
        Value::Array(items) => {
            if !items.is_empty() {
                list(out, items.len());
                for item in items {
                    stack::grow(|| write_value(out, item));
                }
            }
            out.push(NIL);
        }
        Value::Object(object) => {
            out.push(MAP);
            out.extend_from_slice(&(object.len() as u32).to_be_bytes());
            for (key, item) in object {
                binary(out, key);
                stack::grow(|| write_value(out, item));
            }
        }
    }
}

/// [`write`] of the JSON `toJSON` writes for a node or mark, written from its fields.
pub fn write_fields<'c>(fields: impl Fields<'c>) -> Vec<u8> {
    let mut out = vec![VERSION];
    write_node(&mut out, fields);
    out
}

fn write_node<'c>(out: &mut Vec<u8>, fields: impl Fields<'c>) {
    out.push(MAP);
    out.extend_from_slice(&(fields.field_count() as u32).to_be_bytes());
    fields.fields(|field| {
        binary(out, field.key());
        match field {
            Field::Type(name) => binary(out, name),
            Field::Attrs(attrs) => write_value_ref(out, attrs),
            Field::Content(node) => {
                list(out, node.children().len());
                for child in node.children() {
                    stack::grow(|| write_node(out, child));
                }
                out.push(NIL);
            }
            Field::Marks(marks) => {
                list(out, marks.len());
                for mark in marks.iter() {
                    write_node(out, mark);
                }
                out.push(NIL);
            }
            // A binary holds UTF-8, which has no lone surrogate.
            Field::Text(text) => binary(out, &text.to_string_lossy()),
        }
    });
}

/// [`write_value`] of a value a chunk holds.
fn write_value_ref(out: &mut Vec<u8>, value: ValueRef) {
    match value.kind() {
        Kind::Null => atom(out, "nil"),
        Kind::Bool(true) => atom(out, "true"),
        Kind::Bool(false) => atom(out, "false"),
        Kind::Number(number) => write_number(out, &number),
        Kind::String(text) => binary(out, text),
        Kind::Array(len) => {
            if len > 0 {
                list(out, len as usize);
                for item in value.items() {
                    stack::grow(|| write_value_ref(out, item));
                }
            }
            out.push(NIL);
        }
        Kind::Object(len) => {
            out.push(MAP);
            out.extend_from_slice(&len.to_be_bytes());
            for (key, item) in value.entries() {
                binary(out, key);
                stack::grow(|| write_value_ref(out, item));
            }
        }
    }
}

/// A list's header, which its items and the empty list that ends it follow.
fn list(out: &mut Vec<u8>, len: usize) {
    out.push(LIST);
    out.extend_from_slice(&(len as u32).to_be_bytes());
}

fn atom(out: &mut Vec<u8>, name: &str) {
    out.push(SMALL_ATOM_UTF8);
    out.push(name.len() as u8);
    out.extend_from_slice(name.as_bytes());
}

fn binary(out: &mut Vec<u8>, text: &str) {
    out.push(BINARY);
    out.extend_from_slice(&(text.len() as u32).to_be_bytes());
    out.extend_from_slice(text.as_bytes());
}

/// JavaScript writes a number as an integer when it can, and Jason reads that as an integer,
/// however large.
fn write_number(out: &mut Vec<u8>, number: &Number) {
    if let Some(value) = safe_integer(number) {
        return integer(out, value.into());
    }
    let double = number.as_f64().unwrap_or(f64::NAN);
    if !double.is_finite() {
        return atom(out, "nil");
    }
    match number_to_string(double).parse::<i128>() {
        // JavaScript writes integers below 10^21 as digits, and larger ones in exponent form.
        Ok(value) => integer(out, value),
        Err(_) => {
            out.push(NEW_FLOAT);
            out.extend_from_slice(&double.to_bits().to_be_bytes());
        }
    }
}

/// The integer a number holds, if JavaScript holds it exactly, as it does every integer up to
/// `Number.MAX_SAFE_INTEGER`.
pub fn safe_integer(number: &Number) -> Option<i64> {
    number
        .as_i64()
        .filter(|integer| integer.unsigned_abs() as f64 <= MAX_SAFE_INTEGER)
}

fn integer(out: &mut Vec<u8>, value: i128) {
    if let Ok(small) = u8::try_from(value) {
        out.push(SMALL_INTEGER);
        out.push(small);
    } else if let Ok(word) = i32::try_from(value) {
        out.push(INTEGER);
        out.extend_from_slice(&word.to_be_bytes());
    } else {
        let digits = value.unsigned_abs().to_le_bytes();
        let length = digits.len() - digits.iter().rev().take_while(|&&byte| byte == 0).count();
        out.push(SMALL_BIG);
        out.push(length as u8);
        out.push(u8::from(value < 0));
        out.extend_from_slice(&digits[..length]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(json: &str) -> Value {
        tarnish::json::from_str(json).expect("JSON")
    }

    /// A term, from what `term_to_binary` writes after the version.
    fn term(bytes: &[u8]) -> Vec<u8> {
        [&[VERSION], bytes].concat()
    }

    fn binary_ext(text: &str) -> Vec<u8> {
        let mut out = Vec::new();
        binary(&mut out, text);
        out
    }

    fn atom_ext(name: &str) -> Vec<u8> {
        let mut out = Vec::new();
        atom(&mut out, name);
        out
    }

    #[test]
    fn reads_what_it_writes() {
        for json in [
            "null",
            "[true,false,0,255,256,-1,2147483647,2147483648,-2147483649,9007199254740991]",
            r#"{"a":[],"b":{},"c":"é","d":[[1,[2]]],"e":0.5,"f":1e300,"g":-0.25}"#,
            "123456789012345678901",
        ] {
            assert!(
                read(&write(&value(json))).expect("a term") == value(json),
                "{json}"
            );
        }
    }

    /// A big integer's digits, as the VM writes it for 2^200 + 1 and its negation.
    #[test]
    fn reads_integers_past_128_bits_as_doubles() {
        let mut digits = vec![0u8; 26];
        digits[0] = 1;
        digits[25] = 1;
        for (sign, expected) in [(0, 1.6069380442589903e60), (1, -1.6069380442589903e60)] {
            let bytes = term(&[&[SMALL_BIG, 26, sign][..], &digits].concat());
            assert_eq!(read(&bytes).expect("a term").as_f64(), Some(expected));
        }
    }

    #[test]
    fn reads_a_list_of_bytes_as_numbers() {
        assert!(read(&term(&[STRING, 0, 3, 1, 2, 255])).expect("a term") == value("[1,2,255]"));
    }

    /// `%{:a => 1, "a" => 2}`, whose atom comes first in term order: JSON keeps the first place
    /// of a key written twice, and its last value, and puts the index `7` before both.
    #[test]
    fn reads_keys_that_write_the_same_as_json_parses_them() {
        let bytes = [
            &[MAP, 0, 0, 0, 3][..],
            &atom_ext("a"),
            &[SMALL_INTEGER, 1],
            &[SMALL_INTEGER, 7],
            &[SMALL_INTEGER, 3],
            &binary_ext("a"),
            &[SMALL_INTEGER, 2],
        ]
        .concat();
        let read = read(&term(&bytes)).expect("a term");
        assert!(read == value(r#"{"a":2,"7":3}"#));
        let keys: Vec<&str> = read
            .as_object()
            .expect("an object")
            .keys()
            .map(|key| key.as_str())
            .collect();
        assert_eq!(keys, ["7", "a"]);
    }

    /// `JSON.parse` of the JSON Jason writes for an ordered object puts its array indices first.
    #[test]
    fn reads_an_ordered_objects_indices_first() {
        let entry = |key: &str| [&[SMALL_TUPLE, 2][..], &binary_ext(key), &[NIL]].concat();
        let bytes = [
            &[MAP, 0, 0, 0, 2][..],
            &atom_ext("__struct__"),
            &atom_ext("Elixir.Jason.OrderedObject"),
            &atom_ext("values"),
            &[LIST, 0, 0, 0, 5],
            &entry("b"),
            &entry("10"),
            &entry("a"),
            &entry("2"),
            &entry("01"),
            &[NIL],
        ]
        .concat();
        let read = read(&term(&bytes)).expect("a term");
        let keys: Vec<&str> = read
            .as_object()
            .expect("an object")
            .keys()
            .map(|key| key.as_str())
            .collect();
        assert_eq!(keys, ["2", "10", "b", "a", "01"]);
    }

    #[test]
    fn reads_an_ordered_object_in_its_order() {
        let entry = |key: &str, value: u8| {
            [
                &[SMALL_TUPLE, 2][..],
                &binary_ext(key),
                &[SMALL_INTEGER, value],
            ]
            .concat()
        };
        let bytes = [
            &[MAP, 0, 0, 0, 2][..],
            &atom_ext("__struct__"),
            &atom_ext("Elixir.Jason.OrderedObject"),
            &atom_ext("values"),
            &[LIST, 0, 0, 0, 2],
            &entry("b", 1),
            &entry("a", 2),
            &[NIL],
        ]
        .concat();
        let read = read(&term(&bytes)).expect("a term");
        let keys: Vec<&str> = read
            .as_object()
            .expect("an object")
            .keys()
            .map(|key| key.as_str())
            .collect();
        assert_eq!(keys, ["b", "a"]);
    }

    #[test]
    fn refuses_what_jason_cannot_encode() {
        let other_struct = [
            &[MAP, 0, 0, 0, 1][..],
            &atom_ext("__struct__"),
            &atom_ext("Elixir.MapSet"),
        ]
        .concat();
        for bytes in [
            // A tuple.
            vec![SMALL_TUPLE, 0],
            // An improper list, `[1 | 2]`.
            vec![LIST, 0, 0, 0, 1, SMALL_INTEGER, 1, SMALL_INTEGER, 2],
            // A binary that isn't UTF-8.
            vec![BINARY, 0, 0, 0, 1, 0xff],
            other_struct,
            // Bytes past the term.
            vec![NIL, NIL],
        ] {
            assert!(read(&term(&bytes)).is_err(), "{bytes:?}");
        }
    }
}
