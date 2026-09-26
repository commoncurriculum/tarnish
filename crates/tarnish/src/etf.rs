//! Erlang's external term format, in which the BEAM makes a whole term, or reads one out, in one
//! call: values as the terms Jason decodes from their JSON, and terms as the JSON Jason encodes
//! them as.

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
    let mut reader = Reader { bytes, at: 0 };
    if reader.byte()? != VERSION {
        return Err(NotJson);
    }
    let value = reader.value()?;
    match reader.at == bytes.len() {
        true => Ok(value),
        false => Err(NotJson),
    }
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

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

/// A map's key: its text, and whether no other key of the map can have that text, which only a
/// string key is sure of.
enum ReadKey {
    Text(Key, bool),
    /// A struct's `__struct__` key.
    Struct,
}

impl<'a> Reader<'a> {
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

    fn value(&mut self) -> Result<Value, NotJson> {
        Ok(match self.byte()? {
            tag @ (SMALL_INTEGER | INTEGER | SMALL_BIG | LARGE_BIG) => {
                Value::Number(self.integer(tag)?.number()?)
            }
            NEW_FLOAT => {
                let bits = u64::from_be_bytes(self.take(8)?.try_into().expect("eight bytes"));
                Value::Number(Number::from_f64(f64::from_bits(bits)).ok_or(NotJson)?)
            }
            BINARY => {
                let length = self.u32()?;
                Value::String(text(self.take(length)?)?.to_owned())
            }
            tag @ (ATOM | SMALL_ATOM | ATOM_UTF8 | SMALL_ATOM_UTF8) => match self.atom(tag)? {
                "nil" => Value::Null,
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                name => Value::String(name.to_owned()),
            },
            NIL => Value::Array(Vec::new()),
            // A list of bytes.
            STRING => {
                let length = self.u16()?;
                Value::Array(
                    self.take(length)?
                        .iter()
                        .map(|&byte| Value::Number(byte.into()))
                        .collect(),
                )
            }
            LIST => {
                let length = self.u32()?;
                let mut items = Vec::with_capacity(length.min(self.bytes.len()));
                for _ in 0..length {
                    items.push(stack::grow(|| self.value())?);
                }
                // An improper list has no JSON.
                if self.byte()? != NIL {
                    return Err(NotJson);
                }
                Value::Array(items)
            }
            MAP => {
                let arity = self.u32()?;
                self.object(arity)?
            }
            _ => return Err(NotJson),
        })
    }

    fn object(&mut self, arity: usize) -> Result<Value, NotJson> {
        let mut object = Map::with_capacity(arity.min(self.bytes.len()));
        // Two strings are never the same key of one map, but an atom or integer key may write
        // as one of them.
        let mut unique = true;
        for _ in 0..arity {
            match self.key()? {
                ReadKey::Text(key, string) => {
                    unique &= string;
                    let value = stack::grow(|| self.value())?;
                    if unique {
                        object.push(key, value);
                    } else {
                        object.insert(key, value);
                    }
                }
                // A struct's map is small, so its keys come in term order, the atom
                // `__struct__` before `values`, and before any key of its own that is a string.
                ReadKey::Struct if arity == 2 && object.is_empty() => return self.ordered_object(),
                ReadKey::Struct => return Err(NotJson),
            }
        }
        Ok(Value::Object(object))
    }

    /// The rest of a `Jason.OrderedObject`: its entries, in order.
    fn ordered_object(&mut self) -> Result<Value, NotJson> {
        let tag = self.byte()?;
        if !matches!(tag, ATOM | SMALL_ATOM | ATOM_UTF8 | SMALL_ATOM_UTF8)
            || self.atom(tag)? != "Elixir.Jason.OrderedObject"
        {
            return Err(NotJson);
        }
        match self.key()? {
            ReadKey::Text(key, false) if key == "values" => {}
            _ => return Err(NotJson),
        }
        let length = match self.byte()? {
            NIL => return Ok(Value::Object(Map::new())),
            LIST => self.u32()?,
            _ => return Err(NotJson),
        };
        let mut object = Map::with_capacity(length.min(self.bytes.len()));
        for _ in 0..length {
            if self.take(2)? != [SMALL_TUPLE, 2] {
                return Err(NotJson);
            }
            let ReadKey::Text(key, _) = self.key()? else {
                return Err(NotJson);
            };
            // A key written twice keeps its first place and its last value, as parsing the
            // JSON Jason writes for it does.
            object.insert(key, stack::grow(|| self.value())?);
        }
        if self.byte()? != NIL {
            return Err(NotJson);
        }
        Ok(Value::Object(object))
    }

    fn key(&mut self) -> Result<ReadKey, NotJson> {
        Ok(match self.byte()? {
            BINARY => {
                let length = self.u32()?;
                ReadKey::Text(text(self.take(length)?)?.into(), true)
            }
            tag @ (ATOM | SMALL_ATOM | ATOM_UTF8 | SMALL_ATOM_UTF8) => match self.atom(tag)? {
                "__struct__" => ReadKey::Struct,
                name => ReadKey::Text(name.into(), false),
            },
            tag @ (SMALL_INTEGER | INTEGER | SMALL_BIG | LARGE_BIG) => {
                ReadKey::Text(self.integer(tag)?.text().into(), false)
            }
            _ => return Err(NotJson),
        })
    }

    fn atom(&mut self, tag: u8) -> Result<&'a str, NotJson> {
        let length = match tag {
            SMALL_ATOM | SMALL_ATOM_UTF8 => self.byte()?.into(),
            _ => self.u16()?,
        };
        let name = self.take(length)?;
        // Latin-1 atoms Jason could encode are ASCII, as every atom the VM writes this way is.
        if matches!(tag, ATOM | SMALL_ATOM) && !name.is_ascii() {
            return Err(NotJson);
        }
        text(name)
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
        Number::from_f64(self.text().parse().map_err(|_| NotJson)?).ok_or(NotJson)
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

/// A binary that holds UTF-8, which is all Jason encodes.
fn text(bytes: &[u8]) -> Result<&str, NotJson> {
    std::str::from_utf8(bytes).map_err(|_| NotJson)
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
