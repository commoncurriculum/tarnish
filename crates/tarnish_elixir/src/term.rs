//! Terms read in place as the JSON Jason encodes them, and values made as the terms Jason
//! decodes from the JSON `JSON.stringify` writes for them.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::ops::Range;

use rustler::types::atom;
use rustler::types::map::MapIterator;
use rustler::{BigInt, Binary, Encoder, Env, Term, TermType};
use tarnish::js::{self, Given, WrittenNumber};
use tarnish::json::{self, Key, Map, Number, Value};
use tarnish::{Fields, Mark, Node, stack};

rustler::atoms! {
    __struct__,
    values,
    ordered_object = "Elixir.Jason.OrderedObject",
}

/// Why a term wasn't read.
pub enum Unread {
    /// A part of it read is one Jason couldn't encode.
    NotJson,
    /// It's more than the reading was to read.
    TooBig,
}

/// Text that reads in about the time a map does.
const TEXT_PER_MAP: usize = 256;

/// The value of a term Jason could encode, reading no more than `limit` maps and lists, each
/// [`TEXT_PER_MAP`] bytes of text counting as one more.
pub fn read(term: Term, limit: usize) -> Result<Value, Unread> {
    Ok(read_json(term, limit, |json| json.value())?.0)
}

/// What `read` makes of the term, read as JSON, reading no more than [`read`] does, and the
/// nodes read from maps that aren't what `toJSON` writes for them.
pub fn read_json<'a, T>(
    term: Term<'a>,
    limit: usize,
    read: impl FnOnce(Json<'a, '_>) -> T,
) -> Result<(T, HashSet<usize>), Unread> {
    let reading = Reading {
        refused: Cell::new(false),
        left: Cell::new(limit),
        keys: RefCell::new(Vec::new()),
        open: RefCell::new(Vec::new()),
        irregular: Cell::new(0),
        irregular_nodes: RefCell::new(HashSet::new()),
        given: RefCell::new(Vec::new()),
    };
    let read = read(Json {
        term,
        reading: &reading,
    });
    if reading.left.get() == 0 {
        Err(Unread::TooBig)
    } else if reading.refused.get() {
        Err(Unread::NotJson)
    } else {
        Ok((read, reading.irregular_nodes.into_inner()))
    }
}

/// The term the external format holds.
pub fn make<'a>(env: Env<'a>, bytes: &[u8]) -> Term<'a> {
    env.binary_to_term(bytes)
        .expect("the external format of a value")
        .0
}

/// A term read as the JSON Jason encodes it: maps with string, atom or integer keys,
/// `Jason.OrderedObject`s, lists, strings, atoms and numbers. A part Jason couldn't encode, and
/// any map or list past the reading's limit, reads as `null`, the reading being over.
#[derive(Clone, Copy)]
pub struct Json<'a, 'r> {
    term: Term<'a>,
    reading: &'r Reading<'a>,
}

pub struct Reading<'a> {
    refused: Cell<bool>,
    /// The maps and lists left to read, none once the limit is reached.
    left: Cell<usize>,
    /// The keys looked up so far, as terms.
    keys: RefCell<Vec<(&'static str, Term<'a>)>>,
    /// The maps being read as nodes and marks, innermost last.
    open: RefCell<Vec<Open>>,
    /// A count of the parts read so far that aren't as `toJSON` writes them. A map is as it
    /// writes it only when the count didn't grow while the map was read.
    irregular: Cell<usize>,
    /// The nodes read from maps that aren't what `toJSON` writes for them, by their ids.
    irregular_nodes: RefCell<HashSet<usize>>,
    /// The keys of the attributes given the nodes and marks being read.
    given: RefCell<Vec<Key>>,
}

/// A map being read as a node or mark, until the reader says what it read.
struct Open {
    /// [`Reading::irregular`] when the map was opened.
    irregular: usize,
    /// The map's size, when its keys are all among those looked up, and strings.
    size: Option<usize>,
    /// The nodes and marks read from its `content` and `marks`.
    children: usize,
    marks: usize,
    /// The rank of the last mark read, and whether the marks came in order of rank.
    rank: usize,
    sorted: bool,
    /// Where the keys of the map read as its `attrs` are in [`Reading::given`].
    attrs: Option<Range<usize>>,
}

impl<'a> Reading<'a> {
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
        attrs: &Map,
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
        let mut given = self.given.borrow_mut();
        let given = match closed.attrs.clone() {
            Some(range) => {
                let keys = &given[range.clone()];
                let same = keys.len() == attrs.len() && attrs.keys().all(|key| keys.contains(key));
                given.truncate(range.start);
                same
            }
            None => attrs.is_empty(),
        };
        closed.size == Some(fields)
            && closed.irregular == self.irregular.get()
            && given
            && more(&closed)
    }

    /// Counts `maps` more read, `false` once there are too many.
    fn count(&self, maps: usize) -> bool {
        let left = self.left.get().saturating_sub(maps);
        self.left.set(left);
        left > 0
    }

    fn key(&self, env: Env<'a>, name: &'static str) -> Term<'a> {
        let mut keys = self.keys.borrow_mut();
        if let Some(&(_, key)) = keys.iter().find(|(known, _)| std::ptr::eq(*known, name)) {
            return key;
        }
        let key = name.encode(env);
        keys.push((name, key));
        key
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
        match term.get_type() {
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
            TermType::Integer => match integer(term) {
                Some((number, written)) => {
                    if !written {
                        self.reading.irregular();
                    }
                    Kind::Number(number)
                }
                None => self.refuse(Kind::Null),
            },
            TermType::Float => match term.decode::<f64>().ok().and_then(Number::from_f64) {
                Some(number) => {
                    if !matches!(js::written_number(&number), WrittenNumber::Float(_)) {
                        self.reading.irregular();
                    }
                    Kind::Number(number)
                }
                None => self.refuse(Kind::Null),
            },
            _ => self.refuse(Kind::Null),
        }
    }

    /// A binary's text, which must be UTF-8. Past the reading's limit, it's none.
    fn text_of(self, binary: Binary<'a>) -> Option<&'a str> {
        let bytes = binary.as_slice();
        if !self.reading.count(bytes.len() / TEXT_PER_MAP) {
            return None;
        }
        std::str::from_utf8(bytes).ok()
    }

    fn string(self) -> Option<&'a str> {
        self.text_of(Binary::from_term(self.term).ok()?)
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

    /// A map key as Jason writes it: a string, an atom's name, or an integer's digits.
    fn key(self, key: Term<'a>) -> Option<Cow<'a, str>> {
        if let Ok(binary) = Binary::from_term(key) {
            return self.text_of(binary).map(Cow::Borrowed);
        }
        Some(match key.get_type() {
            TermType::Atom => Cow::Owned(atom_name(key)?),
            TermType::Integer => Cow::Owned(match key.decode::<i64>() {
                Ok(integer) => integer.to_string(),
                Err(_) => key.decode::<BigInt>().ok()?.to_string(),
            }),
            _ => return None,
        })
    }

    /// The fields of these names, and the map's size when it has no keys but those found.
    fn find<const N: usize>(self, keys: [&'static str; N]) -> ([Option<Self>; N], Option<usize>) {
        let mut fields = [None; N];
        let Ok(size) = self.term.map_size() else {
            return (fields, None);
        };
        if !self.reading.count(1) {
            return (fields, None);
        }
        // A map with no keys but these, as strings, has each looked up. Any other key might be
        // an atom or integer that Jason writes as one of them.
        let mut found = 0;
        for (field, key) in fields.iter_mut().zip(keys) {
            if found == size {
                break;
            }
            let key = self.reading.key(self.term.get_env(), key);
            if let Ok(value) = self.term.map_get(key) {
                *field = Some(self.at(value));
                found += 1;
            }
        }
        if found == size {
            return (fields, Some(size));
        }
        fields = [None; N];
        // Of keys that write the same, the last written is the one that counts.
        for (name, value) in self.entries() {
            if let Some(index) = keys.iter().position(|&key| key == name) {
                fields[index] = Some(value);
            }
        }
        (fields, None)
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
            Kind::Array if self.reading.count(1) => Value::Array(
                self.list()
                    .map(|item| stack::grow(|| item.value()))
                    .collect(),
            ),
            Kind::Array => Value::Null,
            Kind::Object => Value::Object(self.object()),
        }
    }

    fn object(self) -> Map {
        let mut object = Map::with_capacity(self.term.map_size().unwrap_or(0));
        if !self.reading.count(1) {
            return object;
        }
        let mut entries = self.entries();
        while let Some((key, value)) = entries.next() {
            let value = stack::grow(|| value.value());
            // A key written twice keeps its first place and its last value, as parsing the JSON
            // Jason writes does.
            match entries.unique {
                true => object.push(Key::from(key), value),
                false => drop(object.insert(Key::from(key), value)),
            }
        }
        object
    }

    /// A list's items, the list being proper.
    fn list(self) -> impl Iterator<Item = Json<'a, 'r>> {
        let mut rest = Some(self.term);
        std::iter::from_fn(move || {
            let list = rest?;
            match list.list_get_cell() {
                Ok((head, tail)) => {
                    rest = Some(tail);
                    Some(self.at(head))
                }
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
    fn open_ordered(&mut self, name: Term<'a>) -> Option<()> {
        let term = self.json.term;
        self.map = None;
        self.unique = false;
        // A struct is written as a plain map.
        self.json.reading.irregular();
        let ordered =
            self.first && term.map_size().is_ok_and(|size| size == 2) && ordered_object() == name;
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
    type Item = (Cow<'a, str>, Json<'a, 'r>);

    fn next(&mut self) -> Option<Self::Item> {
        let (key, value) = match self.pairs {
            Some(rest) => self.next_pair(rest)?,
            None => {
                let (key, value) = self.map.as_mut()?.next()?;
                if __struct__() == key {
                    self.open_ordered(value)?;
                    return self.next();
                }
                self.first = false;
                (key, value)
            }
        };
        match self.json.key(key) {
            Some(key) => {
                // Only a string key is written as it is.
                if let Cow::Owned(_) = key {
                    self.unique = false;
                    self.json.reading.irregular();
                }
                Some((key, self.json.at(value)))
            }
            None => self.refuse(),
        }
    }
}

impl<'a> js::Json<'a> for Json<'a, '_> {
    fn truthy(self) -> bool {
        match self.kind() {
            Kind::Null => false,
            Kind::Bool(boolean) => boolean,
            Kind::Number(number) => js::truthy(Some(&Value::Number(number))),
            Kind::String(text) => !text.is_empty(),
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

    fn string(self) -> Cow<'a, str> {
        match self.kind_of_string() {
            Kind::String(text) => text,
            _ => Cow::Owned(js::to_string(&self.value())),
        }
    }

    fn text(self) -> Option<Cow<'a, str>> {
        match self.kind_of_string() {
            Kind::String(text) => Some(text),
            _ => None,
        }
    }

    fn items(self) -> Option<impl Iterator<Item = Self>> {
        (self.term.is_list() && self.reading.count(1)).then(|| self.list())
    }

    fn attrs(self) -> Given<'a> {
        match self.kind() {
            Kind::Object => {
                let attrs = self.object();
                if let Some(open) = self.reading.open.borrow_mut().last_mut() {
                    let mut given = self.reading.given.borrow_mut();
                    let start = given.len();
                    given.extend(attrs.keys().cloned());
                    open.attrs = Some(start..given.len());
                }
                Given::Object(Cow::Owned(attrs))
            }
            _ if js::Json::truthy(self) => Given::Object(Cow::Borrowed(&json::EMPTY)),
            _ => Given::Falsy(self.value()),
        }
    }

    fn read_node(self, node: &Node) {
        let reading = self.reading;
        let regular = reading.close(
            node.field_count(),
            node.attrs(),
            |open| {
                open.children == node.child_count()
                    && open.marks == node.marks().len()
                    && open.sorted
            },
            |parent| parent.children += 1,
        );
        if !regular {
            reading.irregular();
            reading.irregular_nodes.borrow_mut().insert(node.id());
        }
    }

    fn read_mark(self, mark: &Mark) {
        let rank = mark.mark_type().rank();
        let regular = self.reading.close(
            mark.field_count(),
            mark.attrs(),
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

/// An integer as JSON reads what Jason writes for it: exactly within 64 bits, and as the
/// nearest double past them. And whether JavaScript writes it back as the same integer.
fn integer(term: Term) -> Option<(Number, bool)> {
    let integer: i128 = match (term.decode::<i64>(), term.decode::<u64>()) {
        (Ok(integer), _) => integer.into(),
        (_, Ok(integer)) => integer.into(),
        _ => {
            let digits = term.decode::<BigInt>().ok()?.to_string();
            return Some((Number::from_f64(digits.parse().ok()?)?, false));
        }
    };
    let number = match i64::try_from(integer) {
        Ok(integer) => Number::from(integer),
        Err(_) => Number::from(integer as u64),
    };
    let written =
        matches!(js::written_number(&number), WrittenNumber::Integer(same) if same == integer);
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
