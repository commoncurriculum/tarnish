//! Nodes, marks and text as a chunk holds them, borrowed: what a walk over a whole document
//! reads, without counting a reference for each node it passes.

use std::borrow::Cow;

use super::schema::{MarkType, NodeType};
use crate::chunk::{
    ASCII, BINDING, Chunk, HELD_AS_UNITS, NODES, Record, TEXT_NODE, ValueRef, value_equals,
};
use crate::js;
use crate::text::{Text, byte_offset};

#[derive(Clone, Copy)]
pub struct NodeRef<'c> {
    pub(crate) chunk: &'c Chunk<'c>,
    pub(crate) id: u32,
    pub(crate) record: Record,
}

impl<'c> NodeRef<'c> {
    #[inline]
    pub(crate) fn at(chunk: &'c Chunk<'c>, id: u32) -> NodeRef<'c> {
        NodeRef {
            chunk,
            id,
            record: chunk.record(id),
        }
    }

    /// The last node a chunk holds, as [`Node::root`](super::Node::root) finds it.
    pub fn root(chunk: &'c Chunk<'c>) -> Option<NodeRef<'c>> {
        let last = chunk.count(NODES).checked_sub(1)?;
        Some(NodeRef::at(chunk, last))
    }

    /// Whether a binding's reading set the node's flag, as
    /// [`Json::read_node`](crate::js::Json::read_node) may.
    #[inline]
    pub fn flagged(self) -> bool {
        self.record.flags & BINDING != 0
    }

    #[inline]
    pub fn node_type(self) -> NodeType<'c> {
        self.chunk
            .schema()
            .node_type_at(usize::from(self.record.ty))
    }

    #[inline]
    pub fn is_text(self) -> bool {
        self.record.flags & TEXT_NODE != 0
    }

    #[inline]
    pub fn child_count(self) -> u32 {
        if self.is_text() { 0 } else { self.record.b }
    }

    /// The size of its content: none for text.
    #[inline]
    pub fn content_size(self) -> u32 {
        if self.is_text() { 0 } else { self.record.size }
    }

    #[inline]
    pub fn node_size(self) -> usize {
        if self.is_text() {
            self.record.size as usize
        } else if self.node_type().is_leaf() {
            1
        } else {
            2 + self.record.size as usize
        }
    }

    /// The chunk that holds the list of the node's kids, where it starts, and the bound of the
    /// kids in that chunk.
    #[inline]
    fn kids(self) -> (&'c Chunk<'c>, u32, u32) {
        self.chunk.kids_of(self.id, self.record.a)
    }

    /// # Panics
    ///
    /// When the node has no child at `index`.
    #[inline]
    pub fn child(self, index: u32) -> NodeRef<'c> {
        assert!(index < self.child_count(), "no child at {index}");
        let (list, start, bound) = self.kids();
        let (chunk, id) = list.child(start, index, bound);
        NodeRef::at(chunk, id)
    }

    pub fn children(self) -> impl DoubleEndedIterator<Item = NodeRef<'c>> + ExactSizeIterator {
        let (list, start, bound) = match self.is_text() {
            true => (self.chunk, 0, 0),
            false => self.kids(),
        };
        list.children(start, self.child_count(), bound)
            .map(|(chunk, id)| NodeRef::at(chunk, id))
    }

    #[inline]
    pub fn attrs(self) -> ValueRef<'c> {
        ValueRef::at(self.chunk, self.record.attrs)
    }

    #[inline]
    pub fn marks(self) -> SetRef<'c> {
        SetRef::at(self.chunk, self.record.marks)
    }

    #[inline]
    pub fn text(self) -> Option<TextRef<'c>> {
        self.is_text().then(|| TextRef::of(self.chunk, self.record))
    }

    pub fn ptr_eq(self, other: NodeRef) -> bool {
        self.id == other.id && self.chunk.ptr_eq(other.chunk)
    }

    /// Whether the nodes have the same type, attributes and marks.
    pub fn same_markup(self, other: NodeRef) -> bool {
        self.node_type() == other.node_type()
            && value_equals(self.attrs(), other.attrs())
            && self.marks().same(other.marks())
    }

    /// Whether the nodes are the same piece of document: the same markup and content.
    pub fn equals(self, other: NodeRef) -> bool {
        if self.ptr_eq(other) {
            return true;
        }
        if !self.same_markup(other) {
            return false;
        }
        match (self.text(), other.text()) {
            (Some(a), Some(b)) => a.same(b),
            (None, None) => {
                self.child_count() == other.child_count()
                    && self
                        .children()
                        .zip(other.children())
                        .all(|(a, b)| crate::stack::grow(|| a.equals(b)))
            }
            _ => false,
        }
    }
}

/// A mark set in a chunk.
#[derive(Clone, Copy)]
pub struct SetRef<'c> {
    pub(crate) chunk: &'c Chunk<'c>,
    pub(crate) set: u32,
}

impl<'c> SetRef<'c> {
    #[inline]
    pub fn at(chunk: &'c Chunk<'c>, reference: u32) -> SetRef<'c> {
        let (chunk, set) = chunk.resolve(reference);
        SetRef { chunk, set }
    }

    #[inline]
    pub fn len(self) -> usize {
        if self.set == 0 {
            return 0;
        }
        self.chunk.set(self.set).1 as usize
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    pub fn iter(self) -> impl DoubleEndedIterator<Item = MarkRef<'c>> + ExactSizeIterator {
        let (start, len) = match self.set {
            0 => (0, 0),
            set => self.chunk.set(set),
        };
        (start..start + len).map(move |member| {
            let (chunk, index) = self.chunk.resolve(self.chunk.member(member));
            MarkRef { chunk, index }
        })
    }

    pub fn ptr_eq(self, other: SetRef) -> bool {
        (self.set == 0 && other.set == 0)
            || (self.set == other.set && self.chunk.ptr_eq(other.chunk))
    }

    /// `Mark.sameSet`.
    pub fn same(self, other: SetRef) -> bool {
        self.ptr_eq(other)
            || (self.len() == other.len()
                && self.iter().zip(other.iter()).all(|(a, b)| a.equals(b)))
    }
}

/// A mark in a chunk.
#[derive(Clone, Copy)]
pub struct MarkRef<'c> {
    pub(crate) chunk: &'c Chunk<'c>,
    pub(crate) index: u32,
}

impl<'c> MarkRef<'c> {
    #[inline]
    pub fn rank(self) -> usize {
        self.chunk.mark(self.index).0 as usize
    }

    pub fn mark_type(self) -> MarkType<'c> {
        self.chunk.schema().mark_type_at(self.rank())
    }

    #[inline]
    pub fn attrs(self) -> ValueRef<'c> {
        ValueRef::at(self.chunk, self.chunk.mark(self.index).1)
    }

    pub fn ptr_eq(self, other: MarkRef) -> bool {
        self.index == other.index && self.chunk.ptr_eq(other.chunk)
    }

    /// `Mark.eq`: the same type, and deeply equal attributes.
    pub fn equals(self, other: MarkRef) -> bool {
        self.ptr_eq(other)
            || (self.mark_type() == other.mark_type() && value_equals(self.attrs(), other.attrs()))
    }
}

/// A text node's text, as its chunk holds it.
#[derive(Clone, Copy)]
pub enum TextRef<'c> {
    /// UTF-8, with its length in UTF-16 units, and whether it is all ASCII.
    Utf8 {
        text: &'c str,
        len: usize,
        ascii: bool,
    },
    /// UTF-16 units, for text with a lone surrogate, as little-endian bytes.
    Units(&'c [u8]),
}

impl<'c> TextRef<'c> {
    pub(crate) fn of(chunk: &'c Chunk<'c>, record: Record) -> TextRef<'c> {
        let (chunk, start) = chunk.resolve(record.a);
        if record.flags & HELD_AS_UNITS != 0 {
            TextRef::Units(chunk.units(start, record.b))
        } else {
            TextRef::Utf8 {
                text: chunk.text(start, record.b),
                len: record.size as usize,
                ascii: record.flags & ASCII != 0,
            }
        }
    }

    /// The length in UTF-16 units.
    pub fn len(self) -> usize {
        match self {
            TextRef::Utf8 { len, .. } => len,
            TextRef::Units(bytes) => bytes.len() / 2,
        }
    }

    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    pub fn as_str(self) -> Option<&'c str> {
        match self {
            TextRef::Utf8 { text, .. } => Some(text),
            TextRef::Units(_) => None,
        }
    }

    /// The UTF-16 unit at `index`, which must be in the text.
    fn unit_at(bytes: &[u8], index: usize) -> u16 {
        u16::from_le_bytes([bytes[index * 2], bytes[index * 2 + 1]])
    }

    pub fn first_unit(self) -> Option<u16> {
        match self {
            TextRef::Utf8 { text, .. } => text.chars().next().map(|first| {
                let mut units = [0; 2];
                first.encode_utf16(&mut units)[0]
            }),
            TextRef::Units(bytes) => (!bytes.is_empty()).then(|| TextRef::unit_at(bytes, 0)),
        }
    }

    pub fn last_unit(self) -> Option<u16> {
        match self {
            TextRef::Utf8 { text, .. } => text.chars().next_back().map(|last| {
                let mut units = [0; 2];
                let units = last.encode_utf16(&mut units);
                units[units.len() - 1]
            }),
            TextRef::Units(bytes) => bytes
                .len()
                .checked_sub(2)
                .map(|_| TextRef::unit_at(bytes, bytes.len() / 2 - 1)),
        }
    }

    pub fn units(self) -> Cow<'c, [u16]> {
        match self {
            TextRef::Utf8 { text, .. } => Cow::Owned(text.encode_utf16().collect()),
            TextRef::Units(bytes) => Cow::Owned(
                bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&pair| u16::from_le_bytes(pair))
                    .collect(),
            ),
        }
    }

    pub fn to_text(self) -> Text {
        match self {
            TextRef::Utf8 { text, .. } => Text::from(text),
            TextRef::Units(_) => Text::from_units(&self.units()),
        }
    }

    pub fn to_string_lossy(self) -> Cow<'c, str> {
        match self {
            TextRef::Utf8 { text, .. } => Cow::Borrowed(text),
            TextRef::Units(_) => Cow::Owned(String::from_utf16_lossy(&self.units())),
        }
    }

    /// The bytes of the UTF-8 text between UTF-16 offsets `from` and `to`, when neither falls
    /// inside a surrogate pair.
    pub(crate) fn byte_range(self, from: usize, to: usize) -> Option<(usize, usize)> {
        match self {
            TextRef::Utf8 { ascii: true, .. } => Some((from, to)),
            TextRef::Utf8 { text, .. } => Some((byte_offset(text, from)?, byte_offset(text, to)?)),
            TextRef::Units(_) => None,
        }
    }

    /// The units from `from` to `to`, clamped, as `String.prototype.slice` takes them.
    pub fn slice(self, from: usize, to: usize) -> Text {
        let to = to.min(self.len());
        let from = from.min(to);
        if let TextRef::Utf8 { text, .. } = self
            && let Some((start, end)) = self.byte_range(from, to)
        {
            return Text::from(&text[start..end]);
        }
        Text::from_units(&self.units()[from..to])
    }

    /// Whether the texts are the same units.
    pub fn same(self, other: TextRef) -> bool {
        match (self, other) {
            (TextRef::Utf8 { text: a, .. }, TextRef::Utf8 { text: b, .. }) => a == b,
            (TextRef::Units(a), TextRef::Units(b)) => a == b,
            _ => false,
        }
    }

    /// `JSON.stringify(text)`.
    pub fn write_json(self, out: &mut String) {
        match self {
            TextRef::Utf8 { text, .. } => js::json::write_string(out, text),
            TextRef::Units(_) => js::json::write_units(out, &self.units()),
        }
    }
}

impl PartialEq<Text> for TextRef<'_> {
    fn eq(&self, other: &Text) -> bool {
        match (self.as_str(), other.as_str()) {
            (Some(a), Some(b)) => a == b,
            (None, None) => *self.units() == *other.units(),
            _ => false,
        }
    }
}

impl std::fmt::Display for TextRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.to_string_lossy())
    }
}

impl std::fmt::Debug for TextRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mut out = String::new();
        self.write_json(&mut out);
        f.write_str(&out)
    }
}
