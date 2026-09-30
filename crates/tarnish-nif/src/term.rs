//! Terms read as JSON values, and values and nodes made into terms, one term at a time on the
//! calling thread. Having the VM write a term out in the external format costs a single call
//! more than this, most of all when calls run at once. What these leave to [`etf`] reads or
//! makes the same term as it would: structs, integer keys, atoms that aren't Latin-1, integers
//! past 64 bits and numbers that aren't safe integers.

use rustler::types::atom;
use rustler::types::map::MapIterator;
use rustler::{Binary, Encoder, Env, NewBinary, Term, TermType};
use tarnish::chunk::{Kind, ValueRef};
use tarnish::js::stack;
use tarnish::js::{self, WrittenNumber};
use tarnish::json::{Key, Map, Number, Value};
use tarnish::{Field, Fields};

use crate::etf::{self, NotJson};

/// Reads terms as the values Jason encodes them as, while they weigh no more than it has left: a
/// term weighs one, a list or a map one more for each item or entry, and a binary one more for
/// each byte. What a term weighs keeps roughly in step with how long working on it takes.
pub struct Reader {
    left: usize,
    /// Whether the weight it may read is bounded, as a light call's is.
    bounded: bool,
    irregular: bool,
}

/// Why a reader gave no value for a term.
pub enum Unread {
    NotJson,
    /// The term weighs more than the reader had left.
    Heavy,
}

impl From<NotJson> for Unread {
    fn from(NotJson: NotJson) -> Self {
        Unread::NotJson
    }
}

impl Reader {
    pub fn new(weight: usize) -> Reader {
        Reader {
            left: weight,
            bounded: weight != usize::MAX,
            irregular: false,
        }
    }

    /// Whether a term read so far isn't what Jason decodes from the JSON it writes for it: an
    /// atom but `nil`, `true` and `false`, a key that isn't a binary, a struct, or a number
    /// JavaScript writes as another. JSON that Jason decoded has none.
    pub fn irregular(&self) -> bool {
        self.irregular
    }

    pub fn read(&mut self, term: Term) -> Result<Value, Unread> {
        let mut value = Value::Null;
        self.read_into(term, &mut value)?;
        Ok(value)
    }

    /// Reads `term` into `slot`, which holds `null`: one read where it is kept isn't copied there
    /// from the stack, which costs a document microseconds.
    fn read_into(&mut self, term: Term, slot: &mut Value) -> Result<(), Unread> {
        self.weigh(1)?;
        *slot = match term.get_type() {
            TermType::Binary => Value::String(text(self.bytes(term)?)?.to_owned()),
            TermType::Atom if atom::nil() == term => return Ok(()),
            TermType::Atom if atom::true_() == term => Value::Bool(true),
            TermType::Atom if atom::false_() == term => Value::Bool(false),
            // The VM gives out an atom's name in Latin-1, which not every name fits.
            TermType::Atom => match term.atom_to_string() {
                Ok(name) => {
                    self.irregular = true;
                    Value::String(name)
                }
                Err(_) => return self.external(term, slot),
            },
            TermType::Integer => match term.decode::<i64>() {
                Ok(integer) => {
                    let number = Number::from(integer);
                    let written = js::written_number(&number);
                    if !matches!(written, WrittenNumber::Integer(same) if same == integer.into()) {
                        self.irregular = true;
                    }
                    Value::Number(number)
                }
                Err(_) => return self.external(term, slot),
            },
            TermType::Float => {
                let double = term.decode::<f64>().map_err(|_| NotJson)?;
                let number = Number::from_f64(double).ok_or(NotJson)?;
                if !matches!(js::written_number(&number), WrittenNumber::Float(_)) {
                    self.irregular = true;
                }
                Value::Number(number)
            }
            TermType::List => return stack::grow(|| self.list(term, slot)),
            TermType::Map => return stack::grow(|| self.object(term, slot)),
            _ => return Err(Unread::NotJson),
        };
        Ok(())
    }

    fn weigh(&mut self, weight: usize) -> Result<(), Unread> {
        self.left = self.left.checked_sub(weight).ok_or(Unread::Heavy)?;
        Ok(())
    }

    fn bytes<'a>(&mut self, term: Term<'a>) -> Result<&'a [u8], Unread> {
        let bytes = Binary::from_term(term).map_err(|_| NotJson)?.as_slice();
        self.weigh(bytes.len())?;
        Ok(bytes)
    }

    /// The VM writes the whole term out before its size is known, however large it is, so a
    /// bounded reading leaves that to an unbounded one.
    fn external(&mut self, term: Term, slot: &mut Value) -> Result<(), Unread> {
        if self.bounded {
            return Err(Unread::Heavy);
        }
        self.irregular = true;
        let bytes = term.to_binary();
        self.weigh(bytes.len())?;
        *slot = etf::read(&bytes)?;
        Ok(())
    }

    fn list(&mut self, term: Term, slot: &mut Value) -> Result<(), Unread> {
        // An improper list has no length, and no JSON.
        let length = term.list_length().map_err(|_| NotJson)?;
        self.weigh(length)?;
        let items = etf::new_array(slot, length);
        let mut rest = term;
        while let Ok((head, tail)) = rest.list_get_cell() {
            self.read_into(head, etf::new_item(items))?;
            rest = tail;
        }
        Ok(())
    }

    fn object(&mut self, map: Term, slot: &mut Value) -> Result<(), Unread> {
        let size = map.map_size().map_err(|_| NotJson)?;
        self.weigh(size)?;
        let object = etf::new_object(slot, size);
        // Two binaries are never the same key of one map, but an atom key may write as one of
        // them. A key written twice keeps its first place and its last value, as parsing the
        // JSON Jason writes for it does.
        let mut unique = true;
        for (key, value) in MapIterator::new(map).ok_or(NotJson)? {
            let key = match key.get_type() {
                TermType::Binary => self::key(self.bytes(key)?)?,
                TermType::Atom if atom::__struct__() != key => match key.atom_to_string() {
                    Ok(name) => {
                        unique = false;
                        self.irregular = true;
                        Key::from(name)
                    }
                    Err(_) => return self.external(map, slot),
                },
                _ => return self.external(map, slot),
            };
            if unique {
                self.read_into(value, etf::new_entry(object, key))?;
            } else {
                let value = self.read(value)?;
                object.insert(key, value);
            }
        }
        etf::in_js_order(slot);
        Ok(())
    }
}

/// A key that is a binary. Those of a node's and a mark's fields, which most of a document's keys
/// are, need no check that they are UTF-8.
fn key(bytes: &[u8]) -> Result<Key, NotJson> {
    Ok(match bytes {
        b"type" => Key::const_new("type"),
        b"attrs" => Key::const_new("attrs"),
        b"content" => Key::const_new("content"),
        b"text" => Key::const_new("text"),
        b"marks" => Key::const_new("marks"),
        _ => Key::from(text(bytes)?),
    })
}

/// A binary's bytes as UTF-8, which is all Jason encodes.
fn text(bytes: &[u8]) -> Result<&str, NotJson> {
    std::str::from_utf8(bytes).map_err(|_| NotJson)
}

/// The term Jason decodes from the JSON `JSON.stringify` writes for the value.
pub fn write<'a>(env: Env<'a>, value: &Value) -> Term<'a> {
    Writer::new(env).write(value)
}

/// A map of at most this many keys is a flatmap, which keeps its keys in term order.
const FLATMAP_KEYS: usize = 32;

/// Makes the terms Jason decodes from the JSON `JSON.stringify` writes. Rustler makes a list from
/// an array of its items, and a map from arrays of its keys and values, which this keeps the
/// items and entries of the lists and maps being made in, each one's above its parent's, so that
/// none needs arrays of its own.
///
/// A list is made of the items given since [`Writer::begin_list`], and a map of the entries
/// given since [`Writer::begin_map`].
pub struct Writer<'a, 'v> {
    env: Env<'a>,
    items: Vec<Term<'a>>,
    /// Each entry's key, and its key's and value's terms.
    entries: Vec<(Text<'v>, Term<'a>, Term<'a>)>,
    /// The keys and values of the map being made.
    keys: Vec<Term<'a>>,
    values: Vec<Term<'a>>,
    /// The binaries made for keys and node types, which a document repeats, so that one each
    /// serves them all. A text takes its slot over from any other.
    made: [Option<(Text<'v>, Term<'a>)>; MADE_SLOTS],
}

impl<'a, 'v> Writer<'a, 'v> {
    pub fn new(env: Env<'a>) -> Self {
        Writer {
            env,
            items: Vec::new(),
            entries: Vec::new(),
            keys: Vec::new(),
            values: Vec::new(),
            made: [None; MADE_SLOTS],
        }
    }

    pub fn write(&mut self, value: &'v Value) -> Term<'a> {
        let env = self.env;
        match value {
            Value::Null => atom::nil().encode(env),
            Value::Bool(boolean) => boolean.encode(env),
            Value::Number(number) => self.number(number),
            Value::String(text) => binary(env, text),
            Value::Array(items) => stack::grow(|| {
                let base = self.begin_list();
                for item in items {
                    let term = self.write(item);
                    self.item(term);
                }
                self.list(base)
            }),
            Value::Object(map) => stack::grow(|| self.object(map)),
        }
    }

    pub fn object(&mut self, map: &'v Map) -> Term<'a> {
        let base = self.begin_map();
        for (key, item) in map {
            let term = match item {
                Value::String(kind) if key == "type" => self.name(kind),
                _ => self.write(item),
            };
            self.entry(key, term);
        }
        self.map(base)
    }

    /// A value a chunk holds, as [`Writer::write`] makes it.
    pub fn value_ref(&mut self, value: ValueRef<'v>) -> Term<'a> {
        let env = self.env;
        match value.kind() {
            Kind::Null => atom::nil().encode(env),
            Kind::Bool(boolean) => boolean.encode(env),
            Kind::Number(number) => self.number(&number),
            Kind::String(text) => binary(env, text),
            Kind::Array(_) => stack::grow(|| {
                let base = self.begin_list();
                for item in value.items() {
                    let term = self.value_ref(item);
                    self.item(term);
                }
                self.list(base)
            }),
            Kind::Object(_) => stack::grow(|| {
                let base = self.begin_map();
                for (key, item) in value.entries() {
                    let term = match item.kind() {
                        Kind::String(kind) if key == "type" => self.name(kind),
                        _ => self.value_ref(item),
                    };
                    self.entry(key, term);
                }
                self.map(base)
            }),
        }
    }

    /// A node's or a mark's JSON, as `toJSON` writes it.
    pub fn fields(&mut self, fields: impl Fields<'v>) -> Term<'a> {
        stack::grow(|| {
            let base = self.begin_map();
            fields.fields(|field| {
                let term = match field {
                    Field::Type(name) => self.name(name),
                    Field::Attrs(attrs) => self.value_ref(attrs),
                    Field::Content(node) => {
                        let items = self.begin_list();
                        for child in node.children() {
                            let term = self.fields(child);
                            self.item(term);
                        }
                        self.list(items)
                    }
                    Field::Marks(marks) => {
                        let items = self.begin_list();
                        for mark in marks.iter() {
                            let term = self.fields(mark);
                            self.item(term);
                        }
                        self.list(items)
                    }
                    // A binary holds UTF-8, which has no lone surrogate.
                    Field::Text(text) => binary(self.env, &text.to_string_lossy()),
                };
                self.entry(field.key(), term);
            });
            self.map(base)
        })
    }

    fn number(&mut self, number: &Number) -> Term<'a> {
        match etf::safe_integer(number) {
            Some(integer) => integer.encode(self.env),
            None => {
                let value = Value::Number(number.clone());
                self.env
                    .binary_to_term(&etf::write(&value))
                    .expect("the external format of a number")
                    .0
            }
        }
    }

    /// A string value's binary.
    pub fn text(&mut self, text: &str) -> Term<'a> {
        binary(self.env, text)
    }

    /// A binary for a key or a type's name, one binary serving each time it's made.
    pub fn name(&mut self, name: &'v str) -> Term<'a> {
        self.made(Text::new(name))
    }

    fn made(&mut self, text: Text<'v>) -> Term<'a> {
        let slot = &mut self.made[text.slot()];
        match *slot {
            Some((made, term)) if made == text => term,
            _ => {
                let term = binary(self.env, text.text);
                *slot = Some((text, term));
                term
            }
        }
    }

    pub fn begin_list(&self) -> usize {
        self.items.len()
    }

    pub fn item(&mut self, term: Term<'a>) {
        self.items.push(term);
    }

    /// The list of the items from `base` on.
    pub fn list(&mut self, base: usize) -> Term<'a> {
        let list = self.items[base..].encode(self.env);
        self.items.truncate(base);
        list
    }

    pub fn begin_map(&self) -> usize {
        self.entries.len()
    }

    pub fn entry(&mut self, key: &'v str, term: Term<'a>) {
        let key = Text::new(key);
        let name = self.made(key);
        self.entries.push((key, name, term));
    }

    /// The map of the entries from `base` on.
    pub fn map(&mut self, base: usize) -> Term<'a> {
        let entries = &mut self.entries[base..];
        // The VM insertion-sorts a flatmap's keys, which takes one comparison a key when they
        // come in order.
        if entries.len() <= FLATMAP_KEYS {
            entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        }
        self.keys.clear();
        self.values.clear();
        for &(_, key, value) in entries.iter() {
            self.keys.push(key);
            self.values.push(value);
        }
        self.entries.truncate(base);
        Term::map_from_term_arrays(self.env, &self.keys, &self.values)
            .expect("keys of a map are unique")
    }
}

/// A binary on the process heap, as `NewBinary` makes one. Encoding a `str` makes one off the
/// heap, which the VM copies onto it when it is short.
fn binary<'a>(env: Env<'a>, text: &str) -> Term<'a> {
    let mut binary = NewBinary::new(env, text.len());
    binary.as_mut_slice().copy_from_slice(text.as_bytes());
    binary.into()
}

/// How many binaries `Writer` keeps for texts it may make again.
const MADE_SLOTS: usize = 128;

/// A key or a type's name, with its first eight bytes as a number, which settles most
/// comparisons.
#[derive(Clone, Copy)]
struct Text<'v> {
    head: u64,
    text: &'v str,
}

impl<'v> Text<'v> {
    fn new(text: &'v str) -> Self {
        let bytes = text.as_bytes();
        // Big-endian, so that heads order as their texts do, and padded with zeros.
        let head = match bytes.first_chunk::<8>() {
            Some(first) => u64::from_be_bytes(*first),
            None => bytes
                .iter()
                .fold(0, |head: u64, &byte| head << 8 | u64::from(byte))
                .checked_shl(8 * (8 - bytes.len() as u32))
                .unwrap_or(0),
        };
        Text { head, text }
    }

    fn slot(self) -> usize {
        let hash = (self.head ^ self.text.len() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        (hash >> (u64::BITS - MADE_SLOTS.trailing_zeros())) as usize
    }
}

impl PartialEq for Text<'_> {
    /// The head holds a text of up to eight bytes whole, and the length tells its zero bytes
    /// from the head's padding.
    fn eq(&self, other: &Self) -> bool {
        self.head == other.head
            && self.text.len() == other.text.len()
            && self.text.as_bytes().get(8..) == other.text.as_bytes().get(8..)
    }
}

impl Eq for Text<'_> {}

/// Binaries' term order, which is their bytes'.
impl Ord for Text<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.head
            .cmp(&other.head)
            .then_with(|| self.text.cmp(other.text))
    }
}

impl PartialOrd for Text<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::Text;

    /// Every string of up to `length` of the pieces.
    fn strings(pieces: &[&str], length: usize) -> Vec<String> {
        let mut all = vec![String::new()];
        let mut last = vec![String::new()];
        for _ in 0..length {
            last = last
                .iter()
                .flat_map(|prefix| pieces.iter().map(move |piece| format!("{prefix}{piece}")))
                .collect();
            all.extend(last.iter().cloned());
        }
        all
    }

    #[test]
    fn texts_compare_as_their_bytes() {
        let texts = strings(&["a", "b", "\0", "é", "type", "attrs0"], 4);
        for a in &texts {
            for b in &texts {
                let (text_a, text_b) = (Text::new(a), Text::new(b));
                assert_eq!(text_a == text_b, a == b, "{a:?} {b:?}");
                assert_eq!(text_a.cmp(&text_b), a.cmp(b), "{a:?} {b:?}");
            }
        }
    }
}
