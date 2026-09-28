//! Reading nodes, fragments and marks from JSON, as their `fromJSON`s do, into one chunk.

use std::sync::Arc;

use super::attrs::Computed;
use super::fields::field_count;
use super::schema::Schema;
use crate::chunk::{
    ASCII, BINDING, Builder, Chunk, EMPTY_OBJECT, EMPTY_SET, HELD_AS_UNITS, Kid, Record, TEXT_NODE,
};
use crate::error::{Error, Result};
use crate::js::{AttrKeys, Given, Json, Keys, ReadMark, ReadNode};
use crate::json::{EMPTY, Value};
use crate::stack;
use crate::text::Text;

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
        let name = name.map_or("undefined".into(), Json::string);
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
        let name = name.map_or("undefined".into(), Json::string);
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
