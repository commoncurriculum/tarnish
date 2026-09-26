//! Erlang's external term format, in which the BEAM makes a whole term, or reads one out, in one
//! call: values as the terms Jason decodes from their JSON, and terms as the JSON Jason encodes
//! them as.

use std::borrow::Cow;

use crate::js::{self, Given, Json};
use crate::json::{Key, Map, Number, Value};
use crate::model::Node;
use crate::stack;

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
    Ok(Document::new(bytes)?.root().value())
}

/// The term Jason decodes from the JSON `JSON.stringify` writes for the value.
pub fn write(value: &Value) -> Vec<u8> {
    let mut out = vec![VERSION];
    write_value(&mut out, value);
    out
}

/// What [`write`] writes for the node's JSON, without making the JSON.
pub fn write_node(node: &Node) -> Vec<u8> {
    let mut out = vec![VERSION];
    write_node_value(&mut out, node);
    out
}

/// A term `term_to_binary` wrote, read in place: its values are found once, so that a map's
/// fields and a list's items can be looked up without reading what lies between them, and a
/// node can be read from it without making its JSON first.
pub struct Document<'a> {
    slots: Vec<Slot<'a>>,
    /// The keys that are integers, as Jason writes them.
    keys: Vec<Key>,
}

/// A value found in a document.
#[derive(Clone, Copy)]
struct Slot<'a> {
    kind: Kind<'a>,
    /// The slot after the value and all it holds.
    next: usize,
}

#[derive(Clone, Copy)]
enum Kind<'a> {
    Null,
    Bool(bool),
    Number(Numeral),
    /// A string, or an atom Jason writes as one.
    String(&'a str),
    /// A key that was an integer: its text in the document's keys.
    Key(usize),
    /// An array, whose items follow.
    Array,
    /// An object, whose keys and values follow in turn. `unique` when no two of its keys can
    /// have the same text, which only string keys are sure of.
    Object {
        unique: bool,
    },
}

impl<'a> Document<'a> {
    pub fn new(bytes: &'a [u8]) -> Result<Document<'a>, NotJson> {
        let mut indexer = Indexer {
            bytes,
            at: 0,
            // A value takes at least two bytes.
            slots: Vec::with_capacity(bytes.len() / 2),
            keys: Vec::new(),
        };
        if indexer.byte()? != VERSION {
            return Err(NotJson);
        }
        indexer.value()?;
        if indexer.at != bytes.len() {
            return Err(NotJson);
        }
        Ok(Document {
            slots: indexer.slots,
            keys: indexer.keys,
        })
    }

    pub fn root(&self) -> Term<'_> {
        Term {
            document: self,
            slot: 0,
        }
    }
}

/// A value in a [`Document`].
#[derive(Clone, Copy)]
pub struct Term<'a> {
    document: &'a Document<'a>,
    slot: usize,
}

impl<'a> Term<'a> {
    fn kind(self) -> Kind<'a> {
        self.document.slots[self.slot].kind
    }

    fn at(self, slot: usize) -> Term<'a> {
        Term {
            document: self.document,
            slot,
        }
    }

    /// The terms directly inside an array or object: its items, or its keys and values in turn.
    fn children(self) -> impl Iterator<Item = Term<'a>> {
        let slots = &self.document.slots;
        let end = slots[self.slot].next;
        let first = Some(self.slot + 1).filter(|&slot| slot < end);
        std::iter::successors(first, move |&slot| {
            Some(slots[slot].next).filter(|&next| next < end)
        })
        .map(move |slot| self.at(slot))
    }

    /// An object's keys and values.
    fn entries(self) -> impl Iterator<Item = (&'a str, Term<'a>)> {
        let mut children = self.children();
        std::iter::from_fn(move || {
            let key = match children.next()?.kind() {
                Kind::String(text) => text,
                Kind::Key(index) => &self.document.keys[index],
                _ => unreachable!("a key is a string"),
            };
            Some((key, children.next()?))
        })
    }

    /// The term's value.
    pub fn value(self) -> Value {
        match self.kind() {
            Kind::Null => Value::Null,
            Kind::Bool(boolean) => Value::Bool(boolean),
            Kind::Number(number) => Value::Number(number.into()),
            Kind::String(text) => Value::String(text.to_owned()),
            Kind::Key(index) => Value::String(self.document.keys[index].to_string()),
            Kind::Array => Value::Array(
                self.children()
                    .map(|item| stack::grow(|| item.value()))
                    .collect(),
            ),
            Kind::Object { unique } => Value::Object(self.object(unique)),
        }
    }

    fn object(self, unique: bool) -> Map {
        let mut object = Map::new();
        for (key, value) in self.entries() {
            let value = stack::grow(|| value.value());
            // A key written twice keeps its first place and its last value, as parsing the JSON
            // Jason writes for it does.
            if unique {
                object.push(key.into(), value);
            } else {
                object.insert(key.into(), value);
            }
        }
        object
    }
}

impl<'a> Json<'a> for Term<'a> {
    fn truthy(self) -> bool {
        match self.kind() {
            Kind::Null => false,
            Kind::Bool(boolean) => boolean,
            Kind::Number(number) => number.truthy(),
            Kind::String(text) => !text.is_empty(),
            Kind::Key(_) | Kind::Array | Kind::Object { .. } => true,
        }
    }

    fn get(self, key: &str) -> Option<Self> {
        let Kind::Object { unique } = self.kind() else {
            return None;
        };
        let mut found = self
            .entries()
            .filter(|&(name, _)| name == key)
            .map(|(_, value)| value);
        // Of keys that write the same, the last written is the one that counts.
        match unique {
            true => found.next(),
            false => found.last(),
        }
    }

    fn string(self) -> Cow<'a, str> {
        match self.kind() {
            Kind::String(text) => Cow::Borrowed(text),
            _ => Cow::Owned(js::to_string(&self.value())),
        }
    }

    fn text(self) -> Option<&'a str> {
        match self.kind() {
            Kind::String(text) => Some(text),
            _ => None,
        }
    }

    fn items(self) -> Option<impl Iterator<Item = Self>> {
        match self.kind() {
            Kind::Array => Some(self.children()),
            _ => None,
        }
    }

    fn attrs(self) -> Given<'a> {
        match self.kind() {
            Kind::Object { unique } => Given::Object(Cow::Owned(self.object(unique))),
            _ if self.truthy() => Given::Object(Cow::Owned(Map::new())),
            _ => Given::Falsy(self.value()),
        }
    }
}

/// Finds the values of a document, one after another.
struct Indexer<'a> {
    bytes: &'a [u8],
    at: usize,
    slots: Vec<Slot<'a>>,
    keys: Vec<Key>,
}

/// A map's key, read.
enum ReadKey {
    /// A string, which no other key of the map can be.
    String,
    /// An atom or integer, which may write as the same text as another of the map's keys.
    Other,
    /// A struct's `__struct__` key.
    Struct,
}

impl<'a> Indexer<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], NotJson> {
        let taken = self.bytes.get(self.at..self.at + count).ok_or(NotJson)?;
        self.at += count;
        Ok(taken)
    }

    fn byte(&mut self) -> Result<u8, NotJson> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<usize, NotJson> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().expect("two bytes")).into())
    }

    fn u32(&mut self) -> Result<usize, NotJson> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().expect("four bytes")) as usize)
    }

    fn leaf(&mut self, kind: Kind<'a>) {
        self.slots.push(Slot {
            kind,
            next: self.slots.len() + 1,
        });
    }

    /// A binary that holds UTF-8, which is all Jason encodes.
    fn text(&mut self, length: usize) -> Result<&'a str, NotJson> {
        std::str::from_utf8(self.take(length)?).map_err(|_| NotJson)
    }

    fn value(&mut self) -> Result<(), NotJson> {
        let slot = self.slots.len();
        self.leaf(Kind::Null);
        let kind = match self.byte()? {
            tag @ (SMALL_INTEGER | INTEGER | SMALL_BIG | LARGE_BIG) => {
                Kind::Number(self.integer(tag)?.number()?)
            }
            NEW_FLOAT => {
                let bits = u64::from_be_bytes(self.take(8)?.try_into().expect("eight bytes"));
                Kind::Number(Numeral::double(f64::from_bits(bits))?)
            }
            BINARY => {
                let length = self.u32()?;
                Kind::String(self.text(length)?)
            }
            tag @ (ATOM | SMALL_ATOM | ATOM_UTF8 | SMALL_ATOM_UTF8) => match self.atom(tag)? {
                "nil" => Kind::Null,
                "true" => Kind::Bool(true),
                "false" => Kind::Bool(false),
                name => Kind::String(name),
            },
            NIL => Kind::Array,
            // A list of bytes.
            STRING => {
                let length = self.u16()?;
                for &byte in self.take(length)? {
                    self.leaf(Kind::Number(Numeral::Signed(byte.into())));
                }
                Kind::Array
            }
            LIST => {
                let length = self.u32()?;
                stack::grow(|| self.list(length))?;
                Kind::Array
            }
            MAP => {
                let arity = self.u32()?;
                stack::grow(|| self.object(arity))?
            }
            _ => return Err(NotJson),
        };
        self.slots[slot] = Slot {
            kind,
            next: self.slots.len(),
        };
        Ok(())
    }

    fn list(&mut self, length: usize) -> Result<(), NotJson> {
        for _ in 0..length {
            self.value()?;
        }
        // An improper list has no JSON.
        match self.byte()? {
            NIL => Ok(()),
            _ => Err(NotJson),
        }
    }

    fn object(&mut self, arity: usize) -> Result<Kind<'a>, NotJson> {
        let mut unique = true;
        for index in 0..arity {
            match self.key()? {
                ReadKey::String => {}
                ReadKey::Other => unique = false,
                // A struct's map is small, so its keys come in term order, the atom
                // `__struct__` before `values`, and before any key of its own that is a string.
                ReadKey::Struct if arity == 2 && index == 0 => return self.ordered_object(),
                ReadKey::Struct => return Err(NotJson),
            }
            self.value()?;
        }
        Ok(Kind::Object { unique })
    }

    /// The rest of a `Jason.OrderedObject`: its entries, in order.
    fn ordered_object(&mut self) -> Result<Kind<'a>, NotJson> {
        for expected in ["Elixir.Jason.OrderedObject", "values"] {
            let tag = self.byte()?;
            if !matches!(tag, ATOM | SMALL_ATOM | ATOM_UTF8 | SMALL_ATOM_UTF8)
                || self.atom(tag)? != expected
            {
                return Err(NotJson);
            }
        }
        let length = match self.byte()? {
            NIL => return Ok(Kind::Object { unique: false }),
            LIST => self.u32()?,
            _ => return Err(NotJson),
        };
        for _ in 0..length {
            if self.take(2)? != [SMALL_TUPLE, 2] {
                return Err(NotJson);
            }
            if let ReadKey::Struct = self.key()? {
                return Err(NotJson);
            }
            self.value()?;
        }
        match self.byte()? {
            NIL => Ok(Kind::Object { unique: false }),
            _ => Err(NotJson),
        }
    }

    /// Reads a map's key, adding its slot unless it is `__struct__`.
    fn key(&mut self) -> Result<ReadKey, NotJson> {
        Ok(match self.byte()? {
            BINARY => {
                let length = self.u32()?;
                let text = self.text(length)?;
                self.leaf(Kind::String(text));
                ReadKey::String
            }
            tag @ (ATOM | SMALL_ATOM | ATOM_UTF8 | SMALL_ATOM_UTF8) => match self.atom(tag)? {
                "__struct__" => ReadKey::Struct,
                name => {
                    self.leaf(Kind::String(name));
                    ReadKey::Other
                }
            },
            tag @ (SMALL_INTEGER | INTEGER | SMALL_BIG | LARGE_BIG) => {
                let text = self.integer(tag)?.text();
                self.keys.push(text.into());
                self.leaf(Kind::Key(self.keys.len() - 1));
                ReadKey::Other
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
            INTEGER => Integer::Word(
                i32::from_be_bytes(self.take(4)?.try_into().expect("four bytes")).into(),
            ),
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

/// A number as JSON reads it: an integer exactly within 64 bits, and as the nearest double past
/// them.
#[derive(Clone, Copy)]
enum Numeral {
    Signed(i64),
    Unsigned(u64),
    Double(f64),
}

impl Numeral {
    fn double(double: f64) -> Result<Numeral, NotJson> {
        match double.is_finite() {
            true => Ok(Numeral::Double(double)),
            false => Err(NotJson),
        }
    }

    fn truthy(self) -> bool {
        match self {
            Numeral::Signed(integer) => integer != 0,
            Numeral::Unsigned(integer) => integer != 0,
            Numeral::Double(double) => double != 0.0,
        }
    }
}

impl From<Numeral> for Number {
    fn from(numeral: Numeral) -> Number {
        match numeral {
            Numeral::Signed(integer) => integer.into(),
            Numeral::Unsigned(integer) => integer.into(),
            Numeral::Double(double) => Number::from_f64(double).expect("a finite double"),
        }
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
    fn number(&self) -> Result<Numeral, NotJson> {
        match *self {
            Integer::Word(word) => Ok(Numeral::Signed(word)),
            Integer::Big { negative, digits } if digits.len() <= 8 => {
                let mut bytes = [0; 8];
                bytes[..digits.len()].copy_from_slice(digits);
                let magnitude = u64::from_le_bytes(bytes);
                match negative {
                    false => Ok(Numeral::Unsigned(magnitude)),
                    true if magnitude <= 1 << 63 => {
                        Ok(Numeral::Signed((magnitude as i64).wrapping_neg()))
                    }
                    true => Numeral::double(self.text().parse().map_err(|_| NotJson)?),
                }
            }
            Integer::Big { .. } => Numeral::double(self.text().parse().map_err(|_| NotJson)?),
        }
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

fn write_value(out: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Null => atom(out, "nil"),
        Value::Bool(true) => atom(out, "true"),
        Value::Bool(false) => atom(out, "false"),
        Value::Number(number) => write_number(out, number),
        Value::String(text) => binary(out, text),
        Value::Array(items) => list(out, items.len(), items, |out, item| {
            stack::grow(|| write_value(out, item))
        }),
        Value::Object(object) => map(out, object),
    }
}

fn write_node_value(out: &mut Vec<u8>, node: &Node) {
    let content = node.children();
    let fields = 1
        + usize::from(!node.attrs().is_empty())
        + usize::from(!content.is_empty())
        + usize::from(!node.marks().is_empty())
        + usize::from(node.text().is_some());
    map_header(out, fields);
    binary(out, "type");
    binary(out, node.node_type().name());
    if !node.attrs().is_empty() {
        binary(out, "attrs");
        map(out, node.attrs());
    }
    if !content.is_empty() {
        binary(out, "content");
        list(out, content.len(), content, |out, child| {
            stack::grow(|| write_node_value(out, child))
        });
    }
    if !node.marks().is_empty() {
        binary(out, "marks");
        list(out, node.marks().len(), node.marks().iter(), |out, mark| {
            map_header(out, 1 + usize::from(!mark.attrs().is_empty()));
            binary(out, "type");
            binary(out, mark.mark_type().name());
            if !mark.attrs().is_empty() {
                binary(out, "attrs");
                map(out, mark.attrs());
            }
        });
    }
    if let Some(text) = node.text() {
        binary(out, "text");
        binary(out, &text.to_string_lossy());
    }
}

fn map_header(out: &mut Vec<u8>, arity: usize) {
    out.push(MAP);
    out.extend_from_slice(&(arity as u32).to_be_bytes());
}

fn map(out: &mut Vec<u8>, object: &Map) {
    map_header(out, object.len());
    for (key, item) in object {
        binary(out, key);
        stack::grow(|| write_value(out, item));
    }
}

fn list<T>(
    out: &mut Vec<u8>,
    length: usize,
    items: impl IntoIterator<Item = T>,
    mut write: impl FnMut(&mut Vec<u8>, T),
) {
    if length > 0 {
        out.push(LIST);
        out.extend_from_slice(&(length as u32).to_be_bytes());
        for item in items {
            write(out, item);
        }
    }
    out.push(NIL);
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
    if let Some(value) = number
        .as_i64()
        .filter(|integer| integer.unsigned_abs() as f64 <= crate::js::MAX_SAFE_INTEGER)
    {
        return integer(out, value.into());
    }
    let double = number.as_f64().unwrap_or(f64::NAN);
    if !double.is_finite() {
        return atom(out, "nil");
    }
    let written = crate::js::number_to_string(double);
    match written.parse::<i128>() {
        // JavaScript writes integers below 10^21 as digits, and larger ones in exponent form.
        Ok(value) => integer(out, value),
        Err(_) => {
            out.push(NEW_FLOAT);
            out.extend_from_slice(&double.to_bits().to_be_bytes());
        }
    }
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

    fn round_trip(json: &str) {
        let value = crate::json::from_str(json).expect("JSON");
        assert!(read(&write(&value)).expect("a term") == value, "{json}");
    }

    #[test]
    fn reads_what_it_writes() {
        for json in [
            "null",
            "[true,false,0,255,256,-1,2147483647,2147483648,-2147483649,9007199254740991]",
            r#"{"a":[],"b":{},"c":"é","d":[[1,[2]]],"e":0.5,"f":1e300,"g":-0.25}"#,
            "123456789012345678901",
        ] {
            round_trip(json);
        }
    }

    /// A big integer's digits, as the VM writes it for 2^200 + 1 and its negation.
    #[test]
    fn reads_integers_past_128_bits_as_doubles() {
        let mut digits = vec![0u8; 26];
        digits[0] = 1;
        digits[25] = 1;
        for (sign, expected) in [(0, 1.6069380442589903e60), (1, -1.6069380442589903e60)] {
            let mut bytes = vec![VERSION, SMALL_BIG, 26, sign];
            bytes.extend_from_slice(&digits);
            let value = read(&bytes).expect("a term");
            assert_eq!(value.as_f64(), Some(expected));
        }
    }
}
