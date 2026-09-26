//! Elixir terms to JSON values, as Jason encodes them.

use std::mem::MaybeUninit;

use rustler::sys::{self, ErlNifMapIterator, ErlNifMapIteratorEntry, ErlNifTermType};
use rustler::types::atom;
use rustler::wrapper::{NIF_ENV, NIF_TERM};
use rustler::{BigInt, Env, Term};

use tarnish::json::{Key, Map, Number, Value};

/// A term Jason refuses to encode.
pub struct NotJson;

/// How many levels [`Reader::to_value`] recurses before it goes on without recursing.
const RECURSION: usize = 64;

/// Reads the terms of the input, every one of which is in the NIF's environment.
pub struct Reader<'a> {
    env: Env<'a>,
}

impl<'a> Reader<'a> {
    pub fn new(env: Env<'a>) -> Self {
        Reader { env }
    }

    /// A term Jason encodes: maps with string, atom or integer keys, lists, strings, atoms and
    /// numbers. It may nest as deeply as the JSON Jason writes for it, which `JSON.parse`
    /// reads, so past a few levels this keeps the maps and lists it is inside in a vector
    /// instead of recursing.
    pub fn to_value(&self, term: Term<'a>) -> Result<Value, NotJson> {
        self.value(term.as_c_arg(), 0)
    }
}

/// A term read: a value, or a map or list whose items are still to read.
enum Read {
    Value(Value),
    Open(Open),
}

/// A map or list with items left to read, and those read so far.
enum Open {
    Map {
        entries: Entries,
        left: usize,
        object: Map,
        /// The key of the entry whose value is being read.
        key: Key,
        /// Whether every key so far was a string. Two strings are never the same key of one
        /// map, but an atom or integer key may write as one of them.
        unique: bool,
    },
    List {
        rest: NIF_TERM,
        left: usize,
        items: Vec<Value>,
    },
}

impl Open {
    fn left(&self) -> usize {
        match *self {
            Open::Map { left, .. } | Open::List { left, .. } => left,
        }
    }

    /// The next item's term, having read its key if it is a map's.
    fn next(&mut self, reader: &Reader) -> Result<NIF_TERM, NotJson> {
        match self {
            Open::Map {
                entries,
                left,
                key,
                unique,
                ..
            } => {
                *left -= 1;
                let (name, item) = entries.next();
                *key = reader.key(name, unique)?;
                Ok(item)
            }
            Open::List { rest, left, .. } => {
                *left -= 1;
                let (head, tail) = reader.cell(*rest);
                *rest = tail;
                Ok(head)
            }
        }
    }

    /// Adds the value of the item `next` gave.
    fn put(&mut self, value: Value) {
        match self {
            Open::Map {
                object,
                key,
                unique,
                ..
            } => {
                let key = std::mem::take(key);
                if *unique {
                    object.push(key, value);
                } else {
                    object.insert(key, value);
                }
            }
            Open::List { items, .. } => items.push(value),
        }
    }

    fn close(self) -> Value {
        match self {
            Open::Map { object, .. } => Value::Object(object),
            Open::List { items, .. } => Value::Array(items),
        }
    }
}

impl<'a> Reader<'a> {
    fn raw(&self) -> NIF_ENV {
        self.env.as_c_arg()
    }

    /// A term's value, `depth` maps and lists down.
    fn value(&self, term: NIF_TERM, depth: usize) -> Result<Value, NotJson> {
        if depth == RECURSION {
            return match self.read(term)? {
                Read::Value(value) => Ok(value),
                Read::Open(open) => self.value_deep(open),
            };
        }
        let env = self.raw();
        // SAFETY: as in `read`.
        match unsafe { sys::enif_term_type(env, term) } {
            ErlNifTermType::ERL_NIF_TERM_TYPE_MAP => {
                let mut size = 0;
                // SAFETY: as in `read`.
                unsafe { sys::enif_get_map_size(env, term, &mut size) };
                let mut object = Map::with_capacity(size);
                if size > 0 {
                    let mut entries = Entries::new(env, term);
                    let mut unique = true;
                    for _ in 0..size {
                        let (name, item) = entries.next();
                        let key = self.key(name, &mut unique)?;
                        let value = self.value(item, depth + 1)?;
                        if unique {
                            object.push(key, value);
                        } else {
                            object.insert(key, value);
                        }
                    }
                }
                Ok(Value::Object(object))
            }
            ErlNifTermType::ERL_NIF_TERM_TYPE_LIST => {
                let mut items = Vec::new();
                let (mut rest, mut head, mut tail) = (term, 0, 0);
                // SAFETY: as in `read`.
                while unsafe { sys::enif_get_list_cell(env, rest, &mut head, &mut tail) } != 0 {
                    items.push(self.value(head, depth + 1)?);
                    rest = tail;
                }
                // SAFETY: as in `read`. An improper list has no JSON.
                if unsafe { sys::enif_is_empty_list(env, rest) } == 0 {
                    return Err(NotJson);
                }
                Ok(Value::Array(items))
            }
            kind => self.scalar(term, kind),
        }
    }

    /// The value of a map or list, and of all it holds, read without recursing.
    fn value_deep(&self, root: Open) -> Result<Value, NotJson> {
        let mut open = vec![root];
        loop {
            let parent = open.last_mut().expect("a map or list with items left");
            match self.read(parent.next(self)?)? {
                Read::Open(container) => open.push(container),
                Read::Value(mut value) => loop {
                    let parent = open.last_mut().expect("the parent");
                    parent.put(value);
                    if parent.left() > 0 {
                        break;
                    }
                    value = open.pop().expect("the parent").close();
                    if open.is_empty() {
                        return Ok(value);
                    }
                },
            }
        }
    }

    /// A scalar's value, or a map or list with items to read. An empty map or list is a value.
    fn read(&self, term: NIF_TERM) -> Result<Read, NotJson> {
        let env = self.raw();
        // SAFETY: every term read is the input's or inside it, in the NIF's environment.
        let value = match unsafe { sys::enif_term_type(env, term) } {
            ErlNifTermType::ERL_NIF_TERM_TYPE_MAP => {
                let mut size = 0;
                // SAFETY: as above.
                unsafe { sys::enif_get_map_size(env, term, &mut size) };
                if size == 0 {
                    Value::Object(Map::new())
                } else {
                    return Ok(Read::Open(Open::Map {
                        entries: Entries::new(env, term),
                        left: size,
                        object: Map::with_capacity(size),
                        key: Key::default(),
                        unique: true,
                    }));
                }
            }
            ErlNifTermType::ERL_NIF_TERM_TYPE_LIST => {
                let mut length = 0;
                // SAFETY: as above. An improper list has no length, and no JSON.
                if unsafe { sys::enif_get_list_length(env, term, &mut length) } == 0 {
                    return Err(NotJson);
                }
                if length == 0 {
                    Value::Array(Vec::new())
                } else {
                    return Ok(Read::Open(Open::List {
                        rest: term,
                        left: length as usize,
                        items: Vec::with_capacity(length as usize),
                    }));
                }
            }
            kind => self.scalar(term, kind)?,
        };
        Ok(Read::Value(value))
    }

    /// The value of a term that is neither a map nor a list.
    fn scalar(&self, term: NIF_TERM, kind: ErlNifTermType) -> Result<Value, NotJson> {
        Ok(match kind {
            ErlNifTermType::ERL_NIF_TERM_TYPE_BITSTRING => Value::String(self.text(term)?.into()),
            // An atom is an immediate term, the same for every use of it.
            ErlNifTermType::ERL_NIF_TERM_TYPE_ATOM => match term {
                atom if atom == atom::nil().as_c_arg() => Value::Null,
                atom if atom == atom::true_().as_c_arg() => Value::Bool(true),
                atom if atom == atom::false_().as_c_arg() => Value::Bool(false),
                _ => Value::String(self.term(term).atom_to_string().map_err(|_| NotJson)?),
            },
            ErlNifTermType::ERL_NIF_TERM_TYPE_INTEGER => {
                Value::Number(number(self.term(term)).ok_or(NotJson)?)
            }
            ErlNifTermType::ERL_NIF_TERM_TYPE_FLOAT => {
                Number::from_f64(self.term(term).decode::<f64>().map_err(|_| NotJson)?)
                    .map_or(Value::Null, Value::Number)
            }
            _ => return Err(NotJson),
        })
    }

    /// A map's key as Jason writes it, clearing `unique` for a key that isn't a string.
    fn key(&self, term: NIF_TERM, unique: &mut bool) -> Result<Key, NotJson> {
        // SAFETY: as in `read`.
        let key = match unsafe { sys::enif_term_type(self.raw(), term) } {
            ErlNifTermType::ERL_NIF_TERM_TYPE_BITSTRING => Key::from(self.text(term)?),
            ErlNifTermType::ERL_NIF_TERM_TYPE_ATOM => {
                *unique = false;
                Key::from(self.term(term).atom_to_string().map_err(|_| NotJson)?)
            }
            ErlNifTermType::ERL_NIF_TERM_TYPE_INTEGER => {
                *unique = false;
                Key::from(number(self.term(term)).ok_or(NotJson)?.to_string())
            }
            _ => return Err(NotJson),
        };
        if key == "__struct__" {
            return Err(NotJson);
        }
        Ok(key)
    }

    /// A proper list's head and tail, which `read` found it to be.
    fn cell(&self, list: NIF_TERM) -> (NIF_TERM, NIF_TERM) {
        let (mut head, mut tail) = (0, 0);
        // SAFETY: as in `read`.
        let made = unsafe { sys::enif_get_list_cell(self.raw(), list, &mut head, &mut tail) };
        assert!(made != 0, "a list with items left");
        (head, tail)
    }

    /// A binary that holds UTF-8, which is all Jason encodes.
    fn text(&self, term: NIF_TERM) -> Result<&'a str, NotJson> {
        let mut binary = MaybeUninit::<sys::ErlNifBinary>::uninit();
        // SAFETY: as in `read`; a binary's bytes stay put for as long as the NIF runs.
        let bytes = unsafe {
            if sys::enif_inspect_binary(self.raw(), term, binary.as_mut_ptr()) == 0 {
                return Err(NotJson);
            }
            let binary = binary.assume_init();
            std::slice::from_raw_parts(binary.data, binary.size)
        };
        // Most binaries are keys and ids, shorter than `is_ascii` needs to pay back its word
        // alignment: OR-ing their bytes is quicker.
        let ascii = if bytes.len() < 32 {
            bytes.iter().fold(0, |any, byte| any | byte) < 0x80
        } else {
            bytes.is_ascii()
        };
        if ascii {
            // SAFETY: ASCII is UTF-8.
            return Ok(unsafe { std::str::from_utf8_unchecked(bytes) });
        }
        std::str::from_utf8(bytes).map_err(|_| NotJson)
    }

    fn term(&self, term: NIF_TERM) -> Term<'a> {
        // SAFETY: as in `read`.
        unsafe { Term::new(self.env, term) }
    }
}

/// A map's entries, read in place: Rustler's `MapIterator` also keeps an iterator from the other
/// end, and costs as much again.
struct Entries {
    env: NIF_ENV,
    iterator: ErlNifMapIterator,
}

impl Entries {
    /// The entries of `map`, a map in `env`.
    fn new(env: NIF_ENV, map: NIF_TERM) -> Entries {
        let mut iterator = MaybeUninit::uninit();
        // SAFETY: `map` is a term in `env`. The iterator holds no pointers into itself, so it
        // may move once made.
        unsafe {
            let made = sys::enif_map_iterator_create(
                env,
                map,
                iterator.as_mut_ptr(),
                ErlNifMapIteratorEntry::ERL_NIF_MAP_ITERATOR_HEAD,
            );
            assert!(made != 0, "a map");
            Entries {
                env,
                iterator: iterator.assume_init(),
            }
        }
    }

    /// The next entry, of which the map has one left.
    fn next(&mut self) -> (NIF_TERM, NIF_TERM) {
        let (mut key, mut value) = (0, 0);
        // SAFETY: the iterator was made in `env`, and isn't destroyed until it is dropped.
        unsafe {
            let made =
                sys::enif_map_iterator_get_pair(self.env, &mut self.iterator, &mut key, &mut value);
            assert!(made != 0, "a map with entries left");
            sys::enif_map_iterator_next(self.env, &mut self.iterator);
        }
        (key, value)
    }
}

impl Drop for Entries {
    fn drop(&mut self) {
        // SAFETY: as in `next`.
        unsafe { sys::enif_map_iterator_destroy(self.env, &mut self.iterator) };
    }
}

/// An integer as JSON reads it: exactly within 64 bits, as a double past them.
fn number(term: Term) -> Option<Number> {
    if let Ok(integer) = term.decode::<i64>() {
        return Some(integer.into());
    }
    if let Ok(integer) = term.decode::<u64>() {
        return Some(integer.into());
    }
    let digits = term.decode::<BigInt>().ok()?.to_string();
    Number::from_f64(digits.parse().ok()?)
}
