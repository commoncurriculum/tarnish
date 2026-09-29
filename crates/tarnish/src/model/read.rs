//! Reading nodes, fragments and marks from JSON, as their `fromJSON`s do, into one chunk: from
//! a [`Value`], or from JSON read in place from another form, as the Elixir binding reads a term.

use std::borrow::Cow;
use std::sync::Arc;

use super::attrs::Computed;
use super::fields::field_count;
use super::schema::Schema;
use crate::chunk::{
    ASCII, BINDING, Builder, Chunk, EMPTY_OBJECT, EMPTY_SET, HELD_AS_UNITS, Kid, Record, TEXT_NODE,
};
use crate::js::text::Text;
use crate::js::{stack, string, truthy};
use crate::json::{EMPTY, Key, Map, Value};
use crate::{Error, Result};

/// Attributes as JavaScript gives them to a type, which reads each as `given && given[name]`.
pub enum Given<'a> {
    /// A falsy value, `null` among them, which every attribute is when the type has no defaults.
    Falsy(Value),
    /// An object's properties. A truthy value that isn't an object has none.
    Object(Cow<'a, Map>),
}

impl<'a> From<Option<&'a Map>> for Given<'a> {
    fn from(attrs: Option<&'a Map>) -> Self {
        attrs.map_or(Given::Falsy(Value::Null), |attrs| {
            Given::Object(Cow::Borrowed(attrs))
        })
    }
}

/// A value as attributes a type is given.
pub fn attrs(value: &Value) -> Given<'_> {
    match value {
        Value::Object(object) => Given::Object(Cow::Borrowed(object)),
        value if truthy(Some(value)) => Given::Object(Cow::Borrowed(&EMPTY)),
        value => Given::Falsy(value.clone()),
    }
}

/// A JSON value as JavaScript reads a node, a fragment or a mark from it: a [`Value`], or JSON
/// read in place from another form, as the Elixir binding reads a term.
pub trait Json<'a>: Copy {
    fn truthy(self) -> bool;

    /// An object's properties of these names; `None` for one it doesn't have, and all of them
    /// for a value that isn't an object.
    fn fields<const N: usize>(self, keys: [&'static str; N]) -> [Option<Self>; N];

    /// `String(value)`.
    fn string(self) -> Result<Cow<'a, str>>;

    /// A string's text; `None` for a value that isn't a string.
    fn text(self) -> Option<Cow<'a, str>>;

    /// An array's items; `None` for a value that isn't an array.
    fn items(self) -> Option<impl Iterator<Item = Self>>;

    /// The value as attributes a type is given, as [`attrs`] reads them.
    fn attrs(self) -> Given<'a>;

    /// Told what was read from the value as a node, after the nodes and marks read from its
    /// parts: whether to set the node's binding flag, which [`NodeRef::flagged`] reads.
    ///
    /// [`NodeRef::flagged`]: crate::model::NodeRef::flagged
    fn read_node(self, _node: &ReadNode) -> bool {
        false
    }

    /// Told what was read from the value as a mark.
    fn read_mark(self, _mark: &ReadMark) {}
}

/// What reading a node's JSON made of it: enough for a binding to tell whether the value read
/// is what `toJSON` writes back.
pub struct ReadNode<'r> {
    /// The node's index in the chunk being read.
    pub index: u32,
    /// How many fields `toJSON` writes for it.
    pub fields: usize,
    pub children: usize,
    pub marks: usize,
    pub attrs: AttrKeys<'r>,
}

/// What reading a mark's JSON made of it.
pub struct ReadMark<'r> {
    pub rank: usize,
    /// How many fields `toJSON` writes for it.
    pub fields: usize,
    pub attrs: AttrKeys<'r>,
}

/// The keys of the attributes `toJSON` writes for a node or mark read.
#[derive(Clone, Copy)]
pub struct AttrKeys<'r>(pub(crate) Keys<'r>);

#[derive(Clone, Copy)]
pub(crate) enum Keys<'r> {
    Map(&'r Map),
    /// Each attribute's value, `None` for one `toJSON` leaves out.
    Values(&'r [(&'r Key, Option<&'r Value>)]),
}

impl<'r> AttrKeys<'r> {
    pub fn iter(self) -> impl Iterator<Item = &'r str> {
        let (map, values) = match self.0 {
            Keys::Map(map) => (Some(map), None),
            Keys::Values(values) => (None, Some(values)),
        };
        let from_map = map
            .into_iter()
            .flat_map(|map| map.keys().map(|key| key.as_str()));
        let from_values = values
            .into_iter()
            .flatten()
            .filter_map(|(key, value)| value.map(|_| key.as_str()));
        from_map.chain(from_values)
    }
}

impl<'a> Json<'a> for &'a Value {
    fn truthy(self) -> bool {
        truthy(Some(self))
    }

    fn fields<const N: usize>(self, keys: [&'static str; N]) -> [Option<Self>; N] {
        let mut fields = [None; N];
        for (name, value) in self.as_object().into_iter().flatten() {
            if let Some(index) = keys.iter().position(|&key| key == name.as_str()) {
                fields[index] = Some(value);
            }
        }
        fields
    }

    fn string(self) -> Result<Cow<'a, str>> {
        string(Some(self))
    }

    fn text(self) -> Option<Cow<'a, str>> {
        self.as_str().map(Cow::Borrowed)
    }

    fn items(self) -> Option<impl Iterator<Item = Self>> {
        Some(self.as_array()?.iter())
    }

    fn attrs(self) -> Given<'a> {
        attrs(self)
    }
}

/// Reads a document into one chunk, sharing what JavaScript shares, or could without it
/// showing: each mark type's mark with its defaults, as the type's `instance`, the set of just
/// that mark, and each node type's default attributes.
pub(crate) struct Reader<'s, 'a> {
    schema: &'s Schema,
    builder: Builder<'a>,
    text_type: u16,
    /// The children of the fragments being read, the innermost's last.
    children: Vec<Kid>,
    /// The marks of the node being read: each one's rank and index.
    marks: Vec<(u32, u32)>,
}

impl<'s, 'a> Reader<'s, 'a> {
    pub fn new(schema: &'s Schema) -> Reader<'s, 'a> {
        Reader {
            schema,
            builder: Builder::new(schema),
            text_type: schema.text_type().index() as u16,
            children: Vec::new(),
            marks: Vec::new(),
        }
    }

    pub fn finish(self) -> Arc<Chunk<'a>> {
        self.builder.seal()
    }

    /// Hands `json`, the value a node was read from, what reading made of it, and flags the
    /// node when `json` asks to.
    fn read<'j>(&mut self, json: impl Json<'j>, read: ReadNode) -> u32 {
        let id = read.index;
        if json.read_node(&read) {
            self.builder.flag(id, BINDING);
        }
        id
    }

    pub fn node<'j>(&mut self, json: impl Json<'j>) -> Result<u32> {
        // The likeliest first, for a reader that looks each up and stops once it has found as
        // many as the object has.
        let fields = json.fields(["type", "text", "content", "marks", "attrs"]);
        // Only a value without these fields can be falsy, which ProseMirror refuses first.
        if fields.iter().all(Option::is_none) && !json.truthy() {
            return Err(Error::Range("Invalid input for Node.fromJSON".into()));
        }
        let [name, text, content, marks, attrs] = fields;
        let (marks, mark_count) = match marks.filter(|marks| marks.truthy()) {
            None => (EMPTY_SET, 0),
            Some(marks) => self.marks(marks)?,
        };
        let name = name.map_or(Ok("undefined".into()), Json::string)?;
        if name == "text" {
            let text = text
                .and_then(Json::text)
                .ok_or_else(|| Error::Range("Invalid text node in JSON".into()))?;
            if text.is_empty() {
                return Err(Error::Range("Empty text nodes are not allowed".into()));
            }
            let index = self.builder.text(self.text_type, marks, &text);
            let read = ReadNode {
                index,
                fields: field_count(false, false, marks != EMPTY_SET, true),
                children: 0,
                marks: mark_count,
                attrs: AttrKeys(Keys::Map(&EMPTY)),
            };
            return Ok(self.read(json, read));
        }
        let (kids_start, kids, size) = match content {
            Some(content) => stack::grow(|| self.fragment(content))?,
            None => (self.children.len(), 0, 0),
        };
        let node_type = match self.schema.expect_node_type(&name) {
            Ok(node_type) => node_type,
            Err(error) => {
                self.children.truncate(kids_start);
                return Err(error);
            }
        };
        let given = attrs.map_or(Given::Falsy(Value::Null), Json::attrs);
        let attr_set = &node_type.data().attrs;
        let computed = attr_set.resolve(&given).and_then(|computed| {
            attr_set.check_computed(&computed)?;
            Ok(computed)
        });
        let computed = match computed {
            Ok(computed) => computed,
            Err(error) => {
                self.children.truncate(kids_start);
                return Err(error);
            }
        };
        let attrs = attr_set.write(&mut self.builder, node_type.index(), &computed);
        let first = self.builder.push_kids(self.children.drain(kids_start..));
        let index = self.builder.element(
            node_type.index() as u16,
            marks,
            attrs,
            first,
            kids as u32,
            size,
        );
        let read = ReadNode {
            index,
            fields: field_count(attrs != EMPTY_OBJECT, size > 0, marks != EMPTY_SET, false),
            children: kids,
            marks: mark_count,
            attrs: attr_set.keys(&computed),
        };
        Ok(self.read(json, read))
    }

    /// Reads a fragment's children onto `children`: where they start there, how many there
    /// are, and their size.
    fn fragment<'j>(&mut self, json: impl Json<'j>) -> Result<(usize, usize, u32)> {
        let start = self.children.len();
        if !json.truthy() {
            return Ok((start, 0, 0));
        }
        let items = json
            .items()
            .ok_or_else(|| Error::Range("Invalid input for Fragment.fromJSON".into()))?;
        let mut size: u64 = 0;
        for item in items {
            match self.node(item) {
                Ok(node) => size += self.push_child(start, node),
                Err(failed) => {
                    self.children.truncate(start);
                    return Err(failed);
                }
            }
        }
        let size = u32::try_from(size)
            .map_err(|_| Error::Range("A fragment holds fewer than 4G positions".into()))?;
        Ok((start, self.children.len() - start, size))
    }

    /// Reads a fragment, as a list of kids of its own: where it starts, its length and size.
    pub fn fragment_list<'j>(&mut self, json: impl Json<'j>) -> Result<(u32, u32, u32)> {
        let (start, count, size) = self.fragment(json)?;
        let kids = self.builder.push_kids(self.children.drain(start..));
        Ok((kids, count as u32, size))
    }

    /// Adds a child to the fragment whose children start at `start`, joined to the text before
    /// it when they have the same marks, as `Fragment.fromArray` joins them. Its size.
    fn push_child(&mut self, start: usize, node: u32) -> u64 {
        let record = self.builder.record(node);
        let size = record.node_size(self.schema) as u64;
        if self.children.len() > start
            && record.flags & TEXT_NODE != 0
            && let Some(&last) = self.children.last()
        {
            let previous = self.builder.record(last.index);
            if previous.flags & TEXT_NODE != 0
                && self.builder.sets_equal(previous.marks, record.marks)
            {
                self.join_text(last.index, previous, record);
                if let Some(last) = self.children.last_mut() {
                    last.size += size as u32;
                }
                return size;
            }
        }
        self.children.push(Kid::local(node, size as u32));
        size
    }

    /// Joins the text node written last, `record`, to `last`, before it.
    fn join_text(&mut self, last: u32, previous: Record, record: Record) {
        if (previous.flags | record.flags) & HELD_AS_UNITS == 0
            && previous.a + previous.b == record.a
            && record.a + record.b == self.builder.text_end()
        {
            // The texts are side by side already.
            let mut joined = previous;
            joined.b += record.b;
            joined.size += record.size;
            joined.flags &= record.flags | !ASCII;
            self.builder.pop_node();
            self.builder.set_record(last, joined);
            return;
        }
        let text: Text = [
            self.builder.text_of_record(previous),
            self.builder.text_of_record(record),
        ]
        .iter()
        .collect();
        self.builder.pop_node();
        let joined = self.builder.text_of(previous.ty, previous.marks, &text);
        let joined = self.builder.record(joined);
        self.builder.pop_node();
        self.builder.set_record(last, joined);
    }

    /// Reads a mark: its index in the chunk.
    pub fn mark<'j>(&mut self, json: impl Json<'j>) -> Result<u32> {
        self.read_mark(json).map(|(_, mark)| mark)
    }

    /// A mark's rank and index.
    fn read_mark<'j>(&mut self, json: impl Json<'j>) -> Result<(u32, u32)> {
        if !json.truthy() {
            return Err(Error::Range("Invalid input for Mark.fromJSON".into()));
        }
        let [name, attrs] = json.fields(["type", "attrs"]);
        let name = name.map_or(Ok("undefined".into()), Json::string)?;
        let mark_type = self
            .schema
            .mark_type(&name)
            .ok_or_else(|| Error::Range(format!("There is no mark type {name} in this schema")))?;
        let rank = mark_type.rank();
        let given = attrs.map_or(Given::Falsy(Value::Null), Json::attrs);
        let attr_set = &mark_type.data().attrs;
        let computed = attr_set.resolve(&given)?;
        attr_set.check_computed(&computed)?;
        let (mark, attrs) = match (&computed, mark_type.default_attrs()) {
            (Computed::Defaults, Some(defaults)) => (
                self.builder.instance(rank, defaults).0,
                !defaults.is_empty(),
            ),
            _ => {
                let attrs = attr_set.write(&mut self.builder, usize::MAX, &computed);
                (self.builder.mark(rank as u32, attrs), attrs != EMPTY_OBJECT)
            }
        };
        json.read_mark(&ReadMark {
            rank,
            fields: field_count(attrs, false, false, false),
            attrs: attr_set.keys(&computed),
        });
        Ok((rank as u32, mark))
    }

    /// A node's marks, as a set: its index, and how many marks it has.
    fn marks<'j>(&mut self, json: impl Json<'j>) -> Result<(u32, usize)> {
        let items = json
            .items()
            .ok_or_else(|| Error::Range("Invalid mark data for Node.fromJSON".into()))?;
        let base = self.marks.len();
        for item in items {
            match self.read_mark(item) {
                Ok(mark) => self.marks.push(mark),
                Err(error) => {
                    self.marks.truncate(base);
                    return Err(error);
                }
            }
        }
        let read = &mut self.marks[base..];
        let count = read.len();
        if let [(rank, mark)] = *read
            && let Some((instance, alone)) = self.builder.instance_of(rank as usize)
            && instance == mark
        {
            self.marks.truncate(base);
            return Ok((alone, count));
        }
        read.sort_by_key(|&(rank, _)| rank);
        let members: Vec<u32> = read.iter().map(|&(_, mark)| mark).collect();
        self.marks.truncate(base);
        Ok((self.builder.set(&members), count))
    }
}
