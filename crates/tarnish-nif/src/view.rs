//! Terms read in place as the JSON Jason encodes them, for tarnish to read a node from without
//! making their values first: a document's parts are read once, into its chunk. What is read
//! weighs as [`Reader`](crate::term::Reader) weighs it, and reads as it reads it.
//!
//! Reading tells tarnish which nodes came from a map that is what `toJSON` writes for them, so
//! that a document's JSON can be that map again.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};

use rustler::types::atom;
use rustler::types::map::MapIterator;
use rustler::{BigInt, Binary, Term, TermType};
use tarnish::js::stack;
use tarnish::js::{self, WrittenNumber};
use tarnish::json::{self, Key, Map, Number, Value};
use tarnish::model::read::{self, AttrKeys, Given, Property, ReadMark, ReadNode};

use crate::term::Unread;

rustler::atoms! {
    __struct__,
    values,
    ordered_object = "Elixir.Jason.OrderedObject",
}

/// What `read` makes of the term, read as JSON, while it weighs no more than `weight`. A node
/// read from a map that isn't what `toJSON` writes for it is flagged.
pub fn read<'a, T>(
    term: Term<'a>,
    weight: usize,
    read: impl FnOnce(Json<'a, '_>) -> T,
) -> Result<T, Unread> {
    let reading = Reading {
        refused: Cell::new(false),
        heavy: Cell::new(false),
        bounded: weight != usize::MAX,
        left: Cell::new(weight),
        open: RefCell::new(Vec::new()),
        irregular: Cell::new(0),
    };
    let read = read(Json {
        term,
        reading: &reading,
    });
    if reading.heavy.get() {
        Err(Unread::Heavy)
    } else if reading.refused.get() {
        Err(Unread::NotJson)
    } else {
        Ok(read)
    }
}

/// A term read as the JSON Jason encodes it: maps with string, atom or integer keys,
/// `Jason.OrderedObject`s, lists, strings, atoms and numbers. A part Jason couldn't encode, and
/// any map or list past the reading's limit, reads as `null`, the reading being over.
#[derive(Clone, Copy)]
pub struct Json<'a, 'r> {
    term: Term<'a>,
    reading: &'r Reading,
}

pub struct Reading {
    refused: Cell<bool>,
    heavy: Cell<bool>,
    /// Whether the weight it may read is bounded, as a light call's is.
    bounded: bool,
    /// The weight left to read.
    left: Cell<usize>,
    /// The maps being read as nodes and marks, innermost last.
    open: RefCell<Vec<Open>>,
    /// A count of the parts read so far that aren't as `toJSON` writes them. A map is as it
    /// writes it only when the count didn't grow while the map was read.
    irregular: Cell<usize>,
}

/// A map being read as a node or mark, until the reader says what it read.
struct Open {
    /// [`Reading::irregular`] when the map was opened.
    irregular: usize,
    /// The map's size, when its keys are all among those looked up.
    size: Option<usize>,
    /// The nodes and marks read from its `content` and `marks`.
    children: usize,
    marks: usize,
    /// The rank of the last mark read, and whether the marks came in order of rank.
    rank: usize,
    sorted: bool,
    /// What the map read as its `attrs` gave the type's attributes.
    attrs: Option<GivenAttrs>,
}

/// What an object read as a node's or mark's attributes gave its type's.
#[derive(Clone, Copy)]
struct GivenAttrs {
    /// How many of the type's attributes it gave.
    given: usize,
    /// Whether it had properties the type has no attributes for.
    others: bool,
}

impl Reading {
    fn irregular(&self) {
        self.irregular.set(self.irregular.get() + 1);
    }

    /// Closes the map just read as a node or mark with these fields and attributes, telling its
    /// parent through `parent`: whether it's what `toJSON` writes, `more` saying of the rest.
    /// A type fills in an attribute a node isn't given, and leaves out one it doesn't have, so
    /// the map read must have the node's attributes and no others.
    fn close(
        &self,
        fields: usize,
        attrs: AttrKeys,
        more: impl FnOnce(&Open) -> bool,
        parent: impl FnOnce(&mut Open),
    ) -> bool {
        let mut open = self.open.borrow_mut();
        let Some(closed) = open.pop() else {
            return false;
        };
        if let Some(open) = open.last_mut() {
            parent(open);
        }
        // Every attribute given is written, so the map holds just those written when it has
        // no others and gave as many.
        let given = match closed.attrs {
            Some(given) => !given.others && attrs.iter().count() == given.given,
            None => attrs.iter().next().is_none(),
        };
        closed.size == Some(fields)
            && closed.irregular == self.irregular.get()
            && given
            && more(&closed)
    }

    /// Whether an integer past 64 bits may be read: its digits take time in proportion to its
    /// size, which a bounded reading leaves to an unbounded one.
    fn big_integer(&self) -> bool {
        if self.bounded {
            self.heavy.set(true);
        }
        !self.bounded
    }

    /// Counts `weight` more read, `false` once it's more than was left, which ends the reading.
    fn weigh(&self, weight: usize) -> bool {
        match self.left.get().checked_sub(weight) {
            Some(left) if !self.heavy.get() => {
                self.left.set(left);
                true
            }
            _ => {
                self.heavy.set(true);
                false
            }
        }
    }
}

/// What a term is, as JSON.
enum Kind<'a> {
    Null,
    Bool(bool),
    Number(Number),
    String(Cow<'a, str>),
    Array,
    /// A map, which may be a `Jason.OrderedObject`.
    Object,
}

impl<'a, 'r> Json<'a, 'r> {
    fn at(self, term: Term<'a>) -> Json<'a, 'r> {
        Json { term, ..self }
    }

    fn refuse<T>(self, instead: T) -> T {
        self.reading.refused.set(true);
        instead
    }

    fn kind(self) -> Kind<'a> {
        let term = self.term;
        let kind = term.get_type();
        if !matches!(kind, TermType::Map | TermType::List | TermType::Binary) {
            self.reading.weigh(1);
        }
        match kind {
            TermType::Map => Kind::Object,
            TermType::List => Kind::Array,
            TermType::Binary => match self.string() {
                Some(text) => Kind::String(Cow::Borrowed(text)),
                None => self.refuse(Kind::Null),
            },
            TermType::Atom if atom::nil() == term => Kind::Null,
            TermType::Atom if atom::true_() == term => Kind::Bool(true),
            TermType::Atom if atom::false_() == term => Kind::Bool(false),
            TermType::Atom => {
                self.reading.irregular();
                match atom_name(term) {
                    Some(name) => Kind::String(Cow::Owned(name)),
                    None => self.refuse(Kind::Null),
                }
            }
            TermType::Integer if !fits_64_bits(term) && !self.reading.big_integer() => Kind::Null,
            TermType::Integer => match integer(term) {
                Some((number, written)) => {
                    if !written {
                        self.reading.irregular();
                    }
                    Kind::Number(number)
                }
                None => self.refuse(Kind::Null),
            },
            TermType::Float => match term.decode::<f64>().ok().map(Number::from) {
                Some(number) => {
                    if !matches!(js::written_number(number), WrittenNumber::Float(_)) {
                        self.reading.irregular();
                    }
                    Kind::Number(number)
                }
                None => self.refuse(Kind::Null),
            },
            _ => self.refuse(Kind::Null),
        }
    }

    /// A binary's text, which must be UTF-8. Past the reading's weight, it's none.
    fn text_of(self, binary: Binary<'a>) -> Option<&'a str> {
        std::str::from_utf8(self.bytes_of(binary)?).ok()
    }

    /// A binary's bytes, weighed. Past the reading's weight, they're none.
    fn bytes_of(self, binary: Binary<'a>) -> Option<&'a [u8]> {
        let bytes = binary.as_slice();
        self.reading.weigh(1 + bytes.len()).then_some(bytes)
    }

    /// A key's text, the reading refused when its bytes aren't UTF-8.
    fn key_text(self, key: EntryKey<'a>) -> Option<Cow<'a, str>> {
        match key {
            EntryKey::Bytes(bytes) => match std::str::from_utf8(bytes) {
                Ok(text) => Some(Cow::Borrowed(text)),
                Err(_) => self.refuse(None),
            },
            EntryKey::Name(name) => Some(Cow::Owned(name)),
        }
    }

    fn string(self) -> Option<&'a str> {
        self.text_of(Binary::from_term(self.term).ok()?)
    }

    /// Whether the map is a struct Jason writes as its `Jason.Encoder` does, which can be any
    /// JSON: every one but `Jason.OrderedObject`. Reading its entries refuses it, as reading it
    /// only as an object doesn't.
    fn is_struct(self) -> bool {
        self.term
            .map_get(__struct__())
            .is_ok_and(|name| ordered_object() != name)
    }

    /// [`kind`](Self::kind), finding a string at once, as the term is likeliest to be.
    fn kind_of_string(self) -> Kind<'a> {
        match Binary::from_term(self.term) {
            Ok(binary) => match self.text_of(binary) {
                Some(text) => Kind::String(Cow::Borrowed(text)),
                None => self.refuse(Kind::Null),
            },
            Err(_) => self.kind(),
        }
    }

    /// A map key that isn't a string, as Jason writes it: an atom's name, or an integer's
    /// digits.
    fn key_name(self, key: Term<'a>) -> Option<String> {
        match key.get_type() {
            TermType::Atom => atom_name(key),
            TermType::Integer => match key.decode::<i64>() {
                Ok(integer) => Some(integer.to_string()),
                Err(_) if self.reading.big_integer() => {
                    Some(key.decode::<BigInt>().ok()?.to_string())
                }
                Err(_) => None,
            },
            _ => None,
        }
    }

    /// The fields of these names, and the map's size when it has no keys but those found.
    fn find<const N: usize>(self, keys: [&'static str; N]) -> ([Option<Self>; N], Option<usize>) {
        let mut fields = [None; N];
        let Ok(size) = self.term.map_size() else {
            return (fields, None);
        };
        if !self.reading.weigh(1 + size) {
            return (fields, None);
        }
        let mut found = 0;
        // Of keys that write the same, the last written is the one that counts.
        for (key, value) in self.entries() {
            match keys.iter().position(|name| key.is(name)) {
                Some(index) => {
                    fields[index] = Some(value);
                    found += 1;
                }
                None => drop(self.key_text(key)),
            }
        }
        (fields, (found == size).then_some(size))
    }

    /// A map's entries, or a `Jason.OrderedObject`'s, as Jason writes them.
    fn entries(self) -> Entries<'a, 'r> {
        Entries {
            json: self,
            map: MapIterator::new(self.term),
            first: true,
            pairs: None,
            unique: true,
        }
    }

    pub fn value(self) -> Value {
        match self.kind() {
            Kind::Null => Value::Null,
            Kind::Bool(boolean) => Value::Bool(boolean),
            Kind::Number(number) => Value::Number(number),
            Kind::String(text) => Value::String(text.into_owned()),
            Kind::Array if self.reading.weigh(1) => Value::Array(
                self.list()
                    .map(|item| stack::grow(|| item.value()))
                    .collect(),
            ),
            Kind::Array => Value::Null,
            Kind::Object => Value::Object(self.object()),
        }
    }

    fn object(self) -> Map {
        let size = self.term.map_size().unwrap_or(0);
        let mut object = Map::with_capacity(size);
        if !self.reading.weigh(1 + size) {
            return object;
        }
        let mut entries = self.entries();
        while let Some((key, value)) = entries.next() {
            let Some(key) = self.key_text(key) else {
                break;
            };
            let value = stack::grow(|| value.value());
            // A key written twice keeps its first place and its last value, as parsing the JSON
            // Jason writes does.
            match entries.unique {
                true => object.push(Key::from(key), value),
                false => drop(object.insert(Key::from(key), value)),
            }
        }
        object.into_js_order()
    }

    /// An object's properties of these names, each in its name's place, and what they gave.
    /// The others are read only to weigh them and see that they're JSON.
    fn named(self, names: &[Key]) -> (Vec<Option<Property<'a>>>, GivenAttrs) {
        let mut values = Vec::new();
        values.resize_with(names.len(), || None);
        let mut given = GivenAttrs {
            given: 0,
            others: false,
        };
        let size = self.term.map_size().unwrap_or(0);
        if !self.reading.weigh(1 + size) {
            return (values, given);
        }
        // Of keys that write the same, the last written is the one that counts.
        for (key, value) in self.entries() {
            match names.iter().position(|name| key.is(name)) {
                Some(index) => {
                    let property = value.property();
                    given.given += usize::from(values[index].replace(property).is_none());
                }
                None => {
                    given.others = true;
                    drop(self.key_text(key));
                    drop(value.value());
                }
            }
        }
        (values, given)
    }

    /// The value as a property, a string's text borrowed from its binary.
    fn property(self) -> Property<'a> {
        match Binary::from_term(self.term) {
            Ok(binary) => match self.text_of(binary) {
                Some(text) => Property::Text(text),
                None => self.refuse(Property::Value(Value::Null)),
            },
            Err(_) => Property::Value(self.value()),
        }
    }

    /// A list's items, the list being proper.
    fn list(self) -> impl Iterator<Item = Json<'a, 'r>> {
        let mut rest = Some(self.term);
        std::iter::from_fn(move || {
            let list = rest?;
            match list.list_get_cell() {
                Ok((head, tail)) if self.reading.weigh(1) => {
                    rest = Some(tail);
                    Some(self.at(head))
                }
                Ok(_) => None,
                Err(_) if list.is_empty_list() => None,
                Err(_) => self.refuse(None),
            }
        })
    }
}

/// A map's entries, or a `Jason.OrderedObject`'s, their keys as Jason writes them.
struct Entries<'a, 'r> {
    json: Json<'a, 'r>,
    /// The map's own entries, while they're the ones read.
    map: Option<MapIterator<'a>>,
    /// Whether none of the map's entries is read yet.
    first: bool,
    /// The `{key, value}` pairs of a `Jason.OrderedObject` left to read.
    pairs: Option<Term<'a>>,
    /// Whether no two keys read so far write the same, as no two strings of a map do.
    unique: bool,
}

impl<'a, 'r> Entries<'a, 'r> {
    /// Stops reading, the rest being no JSON.
    fn refuse<T>(&mut self) -> Option<T> {
        self.map = None;
        self.pairs = None;
        self.json.refuse(None)
    }

    /// Turns to the pairs of the `Jason.OrderedObject` the map is, whose `__struct__` names
    /// `name`. A struct's map is small, so its keys come in term order, `__struct__` first.
    fn open_ordered(&mut self, first: bool, name: Term<'a>) -> Option<()> {
        let term = self.json.term;
        self.map = None;
        self.unique = false;
        // A struct is written as a plain map.
        self.json.reading.irregular();
        let ordered =
            first && term.map_size().is_ok_and(|size| size == 2) && ordered_object() == name;
        match term.map_get(values()) {
            Ok(pairs) if ordered => {
                self.pairs = Some(pairs);
                Some(())
            }
            _ => self.refuse(),
        }
    }

    fn next_pair(&mut self, rest: Term<'a>) -> Option<(Term<'a>, Term<'a>)> {
        match rest.list_get_cell() {
            Ok((head, tail)) => {
                self.pairs = Some(tail);
                head.decode().ok().or_else(|| self.refuse())
            }
            Err(_) if rest.is_empty_list() => None,
            Err(_) => self.refuse(),
        }
    }
}

impl<'a, 'r> Iterator for Entries<'a, 'r> {
    type Item = (EntryKey<'a>, Json<'a, 'r>);

    fn next(&mut self) -> Option<Self::Item> {
        let from_map = self.pairs.is_none();
        let (key, value) = match self.pairs {
            Some(rest) => self.next_pair(rest)?,
            None => self.map.as_mut()?.next()?,
        };
        let first = from_map && std::mem::replace(&mut self.first, false);
        // A string key, the likeliest, is the only one written as it is.
        let key = match Binary::from_term(key) {
            Ok(binary) => match self.json.bytes_of(binary) {
                Some(bytes) => EntryKey::Bytes(bytes),
                None => return self.refuse(),
            },
            Err(_) if from_map && __struct__() == key => {
                self.open_ordered(first, value)?;
                return self.next();
            }
            Err(_) => match self.json.key_name(key) {
                Some(name) => {
                    self.unique = false;
                    self.json.reading.irregular();
                    EntryKey::Name(name)
                }
                None => return self.refuse(),
            },
        };
        Some((key, self.json.at(value)))
    }
}

/// A map key as Jason writes it: a string's bytes, which may not be UTF-8, or the name it
/// writes for an atom or an integer. A key the same as a name is known to be UTF-8, so only
/// the others are checked, as they're read.
enum EntryKey<'a> {
    Bytes(&'a [u8]),
    Name(String),
}

impl EntryKey<'_> {
    fn is(&self, name: &str) -> bool {
        let key = match self {
            EntryKey::Bytes(bytes) => bytes,
            EntryKey::Name(key) => key.as_bytes(),
        };
        key == name.as_bytes()
    }
}

impl<'a> read::Json<'a> for Json<'a, '_> {
    fn truthy(self) -> bool {
        match self.kind() {
            Kind::Null => false,
            Kind::Bool(boolean) => boolean,
            Kind::Number(number) => js::truthy(Some(&Value::Number(number))),
            Kind::String(text) => !text.is_empty(),
            Kind::Object if self.is_struct() => self.refuse(false),
            Kind::Array | Kind::Object => true,
        }
    }

    fn fields<const N: usize>(self, keys: [&'static str; N]) -> [Option<Self>; N] {
        let irregular = self.reading.irregular.get();
        let (fields, size) = self.find(keys);
        self.reading.open.borrow_mut().push(Open {
            irregular,
            size,
            children: 0,
            marks: 0,
            rank: 0,
            sorted: true,
            attrs: None,
        });
        fields
    }

    fn string(self) -> tarnish::Result<Cow<'a, str>> {
        match self.kind_of_string() {
            Kind::String(text) => Ok(text),
            _ => js::to_string(&self.value()).map(Cow::Owned),
        }
    }

    fn text(self) -> Option<Cow<'a, str>> {
        match self.kind_of_string() {
            Kind::String(text) => Some(text),
            Kind::Object if self.is_struct() => self.refuse(None),
            _ => None,
        }
    }

    fn items(self) -> Option<impl Iterator<Item = Self>> {
        (self.term.is_list() && self.reading.weigh(1)).then(|| self.list())
    }

    fn attrs(self, names: &[Key]) -> Given<'a> {
        match self.kind() {
            Kind::Object => {
                let (values, given) = self.named(names);
                if let Some(open) = self.reading.open.borrow_mut().last_mut() {
                    open.attrs = Some(given);
                }
                Given::Named(values)
            }
            _ if read::Json::truthy(self) => Given::Object(Cow::Borrowed(&json::EMPTY)),
            _ => Given::Falsy(self.value()),
        }
    }

    /// Flags the node when the map isn't what `toJSON` writes for it.
    fn read_node(self, node: &ReadNode) -> bool {
        let regular = self.reading.close(
            node.fields,
            node.attrs,
            |open| open.children == node.children && open.marks == node.marks && open.sorted,
            |parent| parent.children += 1,
        );
        if !regular {
            self.reading.irregular();
        }
        !regular
    }

    fn read_mark(self, mark: &ReadMark) {
        let rank = mark.rank;
        let regular = self.reading.close(
            mark.fields,
            mark.attrs,
            |_| true,
            |node| {
                node.marks += 1;
                node.sorted &= rank >= node.rank;
                node.rank = rank;
            },
        );
        if !regular {
            self.reading.irregular();
        }
    }
}

fn fits_64_bits(term: Term) -> bool {
    term.decode::<i64>().is_ok() || term.decode::<u64>().is_ok()
}

/// An integer as JSON reads what Jason writes for it: the nearest double, which past a
/// double's range is ±Infinity. And whether JavaScript writes it back as the same integer.
fn integer(term: Term) -> Option<(Number, bool)> {
    let integer: i128 = match (term.decode::<i64>(), term.decode::<u64>()) {
        (Ok(integer), _) => integer.into(),
        (_, Ok(integer)) => integer.into(),
        _ => {
            let digits = term.decode::<BigInt>().ok()?.to_string();
            return Some((Number::from(digits.parse::<f64>().ok()?), false));
        }
    };
    let number = Number::from(integer as f64);
    let written =
        matches!(js::written_number(number), WrittenNumber::Integer(same) if same == integer);
    Some((number, written))
}

/// An atom's name, which Jason writes as a string.
fn atom_name(term: Term) -> Option<String> {
    term.atom_to_string().ok().or_else(|| {
        // An atom Latin-1 can't hold, which the external format writes in UTF-8.
        match term.to_binary().as_slice() {
            [131, 118, _, _, name @ ..] | [131, 119, _, name @ ..] => {
                String::from_utf8(name.to_vec()).ok()
            }
            _ => None,
        }
    })
}
