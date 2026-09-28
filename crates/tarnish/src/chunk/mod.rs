//! Documents as chunks: immutable, position-independent arrays of fixed-size records, read in
//! place.
//!
//! A chunk holds nodes, the lists of their children, mark sets, marks, attribute values and text,
//! each kind packed in its own section and referred to by index. A chunk may refer into the
//! chunks it imports, which is how a changed document shares what it didn't change: a step
//! writes only the nodes it makes, into a new chunk that imports the old one.
//!
//! The bytes are the whole of a chunk, so a binding can keep them where it likes, as an Erlang
//! binary for one, and read them without copying. Every read is checked against the sections'
//! bounds, and a node's children, and a value's items, come before it, so bad bytes give a
//! panic and never a loop.
//!
//! | section  | element                                            | bytes |
//! |----------|----------------------------------------------------|-------|
//! | externs  | an import's slot, and an index in it               | 8     |
//! | nodes    | [`Record`]                                         | 24    |
//! | kids     | a node: an import's slot, or all ones for this chunk, its index, and its size | 12 |
//! | sets     | a set's first member, and its size                 | 8     |
//! | members  | a mark ref                                         | 4     |
//! | marks    | a mark type's rank, and an attrs value ref         | 8     |
//! | values   | a JSON value: its tag and two words                | 12    |
//! | elements | an array value's item, a value index               | 4     |
//! | entries  | an object value's key, as a span of strings, and its value's index | 12 |
//! | text     | text nodes' UTF-8                                  | 1     |
//! | units    | the UTF-16 units of text that has a lone surrogate | 2     |
//! | strings  | attribute strings and keys, UTF-8                  | 1     |
//! | imports  | an imported chunk's id                             | 8     |
//!
//! Imports come last, so that a change made in place, to a chunk only it holds, can import
//! another chunk by adding to the end.

mod build;
mod value;

use std::borrow::Cow;
use std::sync::Arc;

pub(crate) use build::Builder;
pub(crate) use value::value_equals;
pub use value::{Kind, ValueRef};

use crate::error::{Error, Result};
use crate::model::Schema;

const MAGIC: [u8; 4] = *b"TRN1";

pub(crate) const EXTERNS: usize = 0;
pub(crate) const NODES: usize = 1;
pub(crate) const KIDS: usize = 2;
pub(crate) const SETS: usize = 3;
pub(crate) const MEMBERS: usize = 4;
pub(crate) const MARKS: usize = 5;
pub(crate) const VALUES: usize = 6;
pub(crate) const ELEMENTS: usize = 7;
pub(crate) const ENTRIES: usize = 8;
pub(crate) const TEXT: usize = 9;
pub(crate) const UNITS: usize = 10;
pub(crate) const STRINGS: usize = 11;
pub(crate) const IMPORTS: usize = 12;
pub(crate) const SECTIONS: usize = 13;

/// Each section's element size in bytes.
pub(crate) const WIDTHS: [usize; SECTIONS] = [8, 24, 12, 8, 4, 8, 12, 4, 12, 1, 2, 1, 8];

/// The magic, a word kept at zero, the schema's fingerprint, the chunk's id, and each section's
/// element count.
pub(crate) const HEADER: usize = 4 + 4 + 8 + 8 + 4 * SECTIONS;

/// A reference to something another chunk holds: the rest is an index into `externs`.
pub(crate) const EXTERN: u32 = 1 << 31;

/// The most imports a change in place gives a chunk, past which the change copies instead.
const IMPORTS_IN_PLACE: usize = 16;

/// A kid's slot for a node in the kid's own chunk. A kid names its node's chunk itself, rather
/// than through `externs`, so that a list copies from one chunk to another kid by kid, as
/// changes copy their parents' lists.
pub(crate) const LOCAL: u32 = u32::MAX;

/// A node in a list of kids: the slot of the chunk that holds it, or [`LOCAL`], its index there,
/// and its size, which positions are found by without reading the node.
#[derive(Clone, Copy)]
pub(crate) struct Kid {
    pub slot: u32,
    pub index: u32,
    pub size: u32,
}

impl Kid {
    pub fn local(index: u32, size: u32) -> Kid {
        Kid {
            slot: LOCAL,
            index,
            size,
        }
    }

    fn to_bytes(self) -> [u8; 12] {
        let mut bytes = [0; 12];
        bytes[..4].copy_from_slice(&self.slot.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.index.to_le_bytes());
        bytes[8..].copy_from_slice(&self.size.to_le_bytes());
        bytes
    }
}

/// A record's flag for text held as UTF-16 units, because it has a lone surrogate.
pub(crate) const HELD_AS_UNITS: u16 = 1;
/// A record's flag for text that is all ASCII, whose offsets in units are offsets in bytes.
pub(crate) const ASCII: u16 = 2;
/// A record's flag for a text node.
pub(crate) const TEXT_NODE: u16 = 4;
/// A record's flag that tarnish sets only where a binding's reading asks it to, and neither
/// reads nor copies: a node written anew doesn't have it.
pub(crate) const BINDING: u16 = 1 << 15;

/// A node: its type's index in the schema, its marks and attributes, and for text its text, or
/// for another node its children. Every chunk's set 0 is the empty set, and its value 0 the
/// empty object.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Record {
    pub ty: u16,
    pub flags: u16,
    /// A set ref.
    pub marks: u32,
    /// A value ref.
    pub attrs: u32,
    /// Text: a ref to where it starts in `text`, or in `units` when held as units. Otherwise a
    /// ref to the first of its kids: in its own chunk, where they are nodes before it, or in an
    /// import, where a change wrote them before it wrote the node.
    pub a: u32,
    /// Text: its length as held. Otherwise its number of kids.
    pub b: u32,
    /// Text: its length in UTF-16 units. Otherwise the size of its content.
    pub size: u32,
}

impl Record {
    pub fn to_bytes(self) -> [u8; 24] {
        let mut bytes = [0; 24];
        bytes[0..2].copy_from_slice(&self.ty.to_le_bytes());
        bytes[2..4].copy_from_slice(&self.flags.to_le_bytes());
        for (at, word) in [self.marks, self.attrs, self.a, self.b, self.size]
            .into_iter()
            .enumerate()
        {
            bytes[4 + at * 4..8 + at * 4].copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }

    fn from_bytes(bytes: &[u8; 24]) -> Record {
        let word = |at: usize| {
            u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        };
        Record {
            ty: u16::from_le_bytes([bytes[0], bytes[1]]),
            flags: u16::from_le_bytes([bytes[2], bytes[3]]),
            marks: word(4),
            attrs: word(8),
            a: word(12),
            b: word(16),
            size: word(20),
        }
    }
}

/// An immutable chunk of a document, and the chunks it imports.
pub struct Chunk<'a> {
    bytes: Cow<'a, [u8]>,
    schema: Schema,
    imports: Vec<Arc<Chunk<'a>>>,
    id: u64,
    /// Where each section starts in `bytes`.
    starts: [u32; SECTIONS],
    counts: [u32; SECTIONS],
}

/// What a chunk's header says: its id and the ids of the chunks it imports, which a binding
/// finds before it loads the chunk.
pub struct Header {
    pub id: u64,
    pub imports: Vec<u64>,
}

#[cold]
#[track_caller]
pub(crate) fn corrupt() -> ! {
    panic!("a corrupt chunk")
}

#[inline]
fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap_or_else(|_| corrupt()))
}

#[inline]
fn double_word(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap_or_else(|_| corrupt()))
}

fn invalid() -> Error {
    Error::Range("Invalid document chunk".into())
}

/// The section counts in a header, where each section starts, and the length they all take.
fn layout(bytes: &[u8]) -> Result<([u32; SECTIONS], [u32; SECTIONS])> {
    if bytes.len() < HEADER || bytes[..4] != MAGIC || word(bytes, 4) != 0 {
        return Err(invalid());
    }
    let mut counts = [0; SECTIONS];
    let mut starts = [0; SECTIONS];
    let mut at = HEADER as u64;
    for section in 0..SECTIONS {
        counts[section] = word(bytes, 24 + section * 4);
        starts[section] = u32::try_from(at).map_err(|_| invalid())?;
        at += u64::from(counts[section]) * WIDTHS[section] as u64;
    }
    if at != bytes.len() as u64 {
        return Err(invalid());
    }
    Ok((starts, counts))
}

impl Header {
    pub fn read(bytes: &[u8]) -> Result<Header> {
        let (starts, counts) = layout(bytes)?;
        let start = starts[IMPORTS] as usize;
        Ok(Header {
            id: double_word(bytes, 16),
            imports: (0..counts[IMPORTS] as usize)
                .map(|slot| double_word(bytes, start + slot * 8))
                .collect(),
        })
    }
}

impl<'a> Chunk<'a> {
    /// A chunk from its bytes, with the chunks its header names as imports, in its order.
    pub fn load(
        bytes: Cow<'a, [u8]>,
        schema: &Schema,
        imports: Vec<Arc<Chunk<'a>>>,
    ) -> Result<Chunk<'a>> {
        let (starts, counts) = layout(&bytes)?;
        if double_word(&bytes, 8) != schema.fingerprint() {
            return Err(Error::Range(
                "Document chunk from a different schema".into(),
            ));
        }
        let named = (0..counts[IMPORTS] as usize)
            .map(|slot| double_word(&bytes, starts[IMPORTS] as usize + slot * 8));
        if imports.len() != counts[IMPORTS] as usize
            || !named.zip(&imports).all(|(id, import)| id == import.id)
            || imports.iter().any(|import| import.schema != *schema)
        {
            return Err(invalid());
        }
        Ok(Chunk {
            id: double_word(&bytes, 16),
            bytes,
            schema: schema.clone(),
            imports,
            starts,
            counts,
        })
    }

    /// A chunk a builder packed, with where it put each section and how many of each there are.
    pub(crate) fn from_parts(
        bytes: Vec<u8>,
        schema: Schema,
        imports: Vec<Arc<Chunk<'a>>>,
        starts: [u32; SECTIONS],
        counts: [u32; SECTIONS],
    ) -> Chunk<'a> {
        Chunk {
            id: double_word(&bytes, 16),
            bytes: Cow::Owned(bytes),
            schema,
            imports,
            starts,
            counts,
        }
    }

    /// The chunk's bytes, which [`Chunk::load`] reads back.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    pub fn imports(&self) -> &[Arc<Chunk<'a>>] {
        &self.imports
    }

    pub(crate) fn count(&self, section: usize) -> u32 {
        self.counts[section]
    }

    /// Where element `index` of a section starts.
    #[inline]
    fn at(&self, section: usize, index: u32) -> usize {
        if index >= self.counts[section] {
            corrupt();
        }
        self.starts[section] as usize + index as usize * WIDTHS[section]
    }

    #[inline]
    fn word_of(&self, section: usize, index: u32, word_index: usize) -> u32 {
        word(&self.bytes, self.at(section, index) + word_index * 4)
    }

    /// The span of a section from element `start`, `len` elements long.
    #[inline]
    fn span(&self, section: usize, start: u32, len: u32) -> &[u8] {
        let end = u64::from(start) + u64::from(len);
        if end > u64::from(self.counts[section]) {
            corrupt();
        }
        let from = self.starts[section] as usize + start as usize * WIDTHS[section];
        &self.bytes[from..from + len as usize * WIDTHS[section]]
    }

    #[inline]
    pub(crate) fn record(&self, id: u32) -> Record {
        let at = self.at(NODES, id);
        Record::from_bytes(
            self.bytes[at..at + 24]
                .try_into()
                .unwrap_or_else(|_| corrupt()),
        )
    }

    /// Kid `index` of the kids section: its slot, which is [`LOCAL`] for a node here, and the
    /// node's index.
    #[inline]
    fn kid(&self, index: u32) -> (u32, u32) {
        let at = self.at(KIDS, index);
        (word(&self.bytes, at), word(&self.bytes, at + 4))
    }

    /// The size of the node at `position` in a list of kids starting at `first`.
    #[inline]
    pub(crate) fn kid_size(&self, first: u32, position: u32) -> u32 {
        let at = self.at(
            KIDS,
            first.checked_add(position).unwrap_or_else(|| corrupt()),
        );
        word(&self.bytes, at + 8)
    }

    /// The sizes of the nodes of `count` kids listed from `first`.
    #[inline]
    pub(crate) fn kid_sizes(&self, first: u32, count: u32) -> impl Iterator<Item = u32> + '_ {
        let kids = self.span(KIDS, first, count).as_chunks::<12>().0;
        kids.iter()
            .map(|kid| u32::from_le_bytes([kid[8], kid[9], kid[10], kid[11]]))
    }

    /// The import slot and index an extern names.
    #[inline]
    fn external(&self, reference: u32) -> (usize, u32) {
        let index = reference & !EXTERN;
        (
            self.word_of(EXTERNS, index, 0) as usize,
            self.word_of(EXTERNS, index, 1),
        )
    }

    /// The chunk a ref points into, and the index there.
    #[inline]
    pub(crate) fn resolve(&self, reference: u32) -> (&Chunk<'a>, u32) {
        if reference & EXTERN == 0 {
            return (self, reference);
        }
        let (slot, index) = self.external(reference);
        (self.imports.get(slot).unwrap_or_else(|| corrupt()), index)
    }

    /// [`resolve`](Self::resolve), to the chunk's owner.
    #[inline]
    pub(crate) fn resolve_shared<'s>(
        this: &'s Arc<Chunk<'a>>,
        reference: u32,
    ) -> (&'s Arc<Chunk<'a>>, u32) {
        if reference & EXTERN == 0 {
            return (this, reference);
        }
        let (slot, index) = this.external(reference);
        (this.imports.get(slot).unwrap_or_else(|| corrupt()), index)
    }

    /// Where the kids of node `id` are, `first` being its kids ref: the chunk that holds their
    /// list, where it starts there, and the bound its kids in that chunk must be below. A list
    /// in an import can't lead back here, since imports are older, so it has no bound.
    #[inline]
    pub(crate) fn kids_of(&self, id: u32, first: u32) -> (&Chunk<'a>, u32, u32) {
        match first & EXTERN {
            0 => (self, first, id),
            _ => {
                let (chunk, start) = self.resolve(first);
                (chunk, start, u32::MAX)
            }
        }
    }

    /// [`kids_of`](Self::kids_of), to the list chunk's owner.
    #[inline]
    pub(crate) fn kids_of_shared<'s>(
        this: &'s Arc<Chunk<'a>>,
        id: u32,
        first: u32,
    ) -> (&'s Arc<Chunk<'a>>, u32, u32) {
        match first & EXTERN {
            0 => (this, first, id),
            _ => {
                let (chunk, start) = Chunk::resolve_shared(this, first);
                (chunk, start, u32::MAX)
            }
        }
    }

    /// A node's kid at `position` among a list starting at `first`, which must be a node before
    /// `bound` when it is in this chunk.
    #[inline]
    pub(crate) fn child(&self, first: u32, position: u32, bound: u32) -> (&Chunk<'a>, u32) {
        match self.kid(first.checked_add(position).unwrap_or_else(|| corrupt())) {
            (LOCAL, index) if index < bound => (self, index),
            (LOCAL, _) => corrupt(),
            (slot, index) => (self.import(slot), index),
        }
    }

    /// The nodes of `count` kids listed from `first`, which must be before `bound` where they
    /// are in this chunk: each one's chunk and index.
    #[inline]
    pub(crate) fn children(
        &self,
        first: u32,
        count: u32,
        bound: u32,
    ) -> impl DoubleEndedIterator<Item = (&Chunk<'a>, u32)> + ExactSizeIterator {
        self.kids(first, count, bound)
            .map(|(chunk, index, _)| (chunk, index))
    }

    /// [`children`](Self::children), with the size each kid holds.
    #[inline]
    pub(crate) fn kids(
        &self,
        first: u32,
        count: u32,
        bound: u32,
    ) -> impl DoubleEndedIterator<Item = (&Chunk<'a>, u32, u32)> + ExactSizeIterator {
        let kids = self.span(KIDS, first, count).as_chunks::<12>().0;
        kids.iter().map(move |kid| {
            let slot = u32::from_le_bytes([kid[0], kid[1], kid[2], kid[3]]);
            let index = u32::from_le_bytes([kid[4], kid[5], kid[6], kid[7]]);
            let size = u32::from_le_bytes([kid[8], kid[9], kid[10], kid[11]]);
            match slot {
                LOCAL if index < bound => (self, index, size),
                LOCAL => corrupt(),
                slot => (&**self.import(slot), index, size),
            }
        })
    }

    #[inline]
    fn import(&self, slot: u32) -> &Arc<Chunk<'a>> {
        self.imports.get(slot as usize).unwrap_or_else(|| corrupt())
    }

    #[inline]
    pub(crate) fn child_shared<'s>(
        this: &'s Arc<Chunk<'a>>,
        first: u32,
        position: u32,
        bound: u32,
    ) -> (&'s Arc<Chunk<'a>>, u32) {
        match this.kid(first.checked_add(position).unwrap_or_else(|| corrupt())) {
            (LOCAL, index) if index < bound => (this, index),
            (LOCAL, _) => corrupt(),
            (slot, index) => (this.import(slot), index),
        }
    }

    /// A set's first member and size.
    #[inline]
    pub(crate) fn set(&self, index: u32) -> (u32, u32) {
        (self.word_of(SETS, index, 0), self.word_of(SETS, index, 1))
    }

    /// A member of a set: a mark ref.
    #[inline]
    pub(crate) fn member(&self, index: u32) -> u32 {
        self.word_of(MEMBERS, index, 0)
    }

    /// A mark's type rank and attrs ref.
    #[inline]
    pub(crate) fn mark(&self, index: u32) -> (u32, u32) {
        (self.word_of(MARKS, index, 0), self.word_of(MARKS, index, 1))
    }

    #[inline]
    pub(crate) fn value(&self, index: u32) -> (u32, u32, u32) {
        let at = self.at(VALUES, index);
        (
            word(&self.bytes, at),
            word(&self.bytes, at + 4),
            word(&self.bytes, at + 8),
        )
    }

    /// Element `index` of an array value `parent`, which comes before it.
    #[inline]
    pub(crate) fn element(&self, parent: u32, index: u32) -> u32 {
        let element = self.word_of(ELEMENTS, index, 0);
        if element >= parent {
            corrupt();
        }
        element
    }

    /// Entry `index` of an object value `parent`: its key and value.
    #[inline]
    pub(crate) fn entry(&self, parent: u32, index: u32) -> (&str, u32) {
        let (key, value) = self.entry_bytes(parent, index);
        (
            std::str::from_utf8(key).unwrap_or_else(|_| corrupt()),
            value,
        )
    }

    /// [`entry`](Self::entry), its key as bytes, which comparing needn't check are UTF-8.
    #[inline]
    pub(crate) fn entry_bytes(&self, parent: u32, index: u32) -> (&[u8], u32) {
        let at = self.at(ENTRIES, index);
        let (start, len, value) = (
            word(&self.bytes, at),
            word(&self.bytes, at + 4),
            word(&self.bytes, at + 8),
        );
        if value >= parent {
            corrupt();
        }
        (self.span(STRINGS, start, len), value)
    }

    #[inline]
    pub(crate) fn string(&self, start: u32, len: u32) -> &str {
        std::str::from_utf8(self.span(STRINGS, start, len)).unwrap_or_else(|_| corrupt())
    }

    #[inline]
    pub(crate) fn text(&self, start: u32, len: u32) -> &str {
        std::str::from_utf8(self.span(TEXT, start, len)).unwrap_or_else(|_| corrupt())
    }

    /// The UTF-16 units from `start`, `len` of them, as little-endian bytes.
    #[inline]
    pub(crate) fn units(&self, start: u32, len: u32) -> &[u8] {
        self.span(UNITS, start, len)
    }

    /// Whether this is the very chunk `other` is.
    #[inline]
    pub fn ptr_eq(&self, other: &Chunk) -> bool {
        std::ptr::eq(self.bytes.as_ptr(), other.bytes.as_ptr())
    }

    /// The import slot and index a ref to another chunk names; `None` for a ref into this one.
    pub(crate) fn external_ref(&self, reference: u32) -> Option<(usize, u32)> {
        (reference & EXTERN != 0).then(|| self.external(reference))
    }

    /// An import, to change in place: `None` unless the chunk holds it alone.
    pub(crate) fn import_mut(&mut self, slot: usize) -> Option<&mut Chunk<'a>> {
        self.imports.get_mut(slot).and_then(Arc::get_mut)
    }

    /// Whether the chunk's bytes are its own, so that it may be changed in place once nothing
    /// else holds it.
    pub(crate) fn is_owned(&self) -> bool {
        matches!(self.bytes, Cow::Owned(_))
    }

    /// Words of the chunk's own bytes written over, at byte `at`.
    fn write_words(&mut self, at: usize, words: &[u32]) {
        let Cow::Owned(bytes) = &mut self.bytes else {
            panic!("a chunk changed in place owns its bytes");
        };
        for (word, bytes) in words.iter().zip(bytes[at..].as_chunks_mut::<4>().0) {
            *bytes = word.to_le_bytes();
        }
    }

    /// The slot of `chunk`, importing it, at the end, when it isn't an import yet: `None` once
    /// the chunk has as many imports as changes in place may give it, which would otherwise
    /// keep every chunk they made alive.
    pub(crate) fn import_in_place(&mut self, chunk: &Arc<Chunk<'a>>) -> Option<u32> {
        if let Some(slot) = self
            .imports
            .iter()
            .position(|import| Arc::ptr_eq(import, chunk))
        {
            return Some(slot as u32);
        }
        if self.imports.len() >= IMPORTS_IN_PLACE {
            return None;
        }
        let Cow::Owned(bytes) = &mut self.bytes else {
            panic!("a chunk changed in place owns its bytes");
        };
        bytes.extend_from_slice(&chunk.id.to_le_bytes());
        let slot = self.imports.len() as u32;
        self.counts[IMPORTS] = slot + 1;
        self.write_words(24 + IMPORTS * 4, &[slot + 1]);
        self.imports.push(chunk.clone());
        Some(slot)
    }

    /// Kid `position` of the list starting at `first` written over.
    pub(crate) fn set_kid(&mut self, first: u32, position: u32, kid: Kid) {
        let at = self.at(
            KIDS,
            first.checked_add(position).unwrap_or_else(|| corrupt()),
        );
        self.write_words(at, &[kid.slot, kid.index, kid.size]);
    }

    /// Node `id`'s size written over.
    pub(crate) fn set_size(&mut self, id: u32, size: u32) {
        let at = self.at(NODES, id);
        self.write_words(at + 20, &[size]);
    }

    /// The chunks this one imports, and theirs, each once, this one's last.
    pub fn closure(this: &Arc<Chunk<'a>>) -> Vec<Arc<Chunk<'a>>> {
        let mut found: Vec<Arc<Chunk<'a>>> = Vec::new();
        let mut stack = vec![(this.clone(), false)];
        while let Some((chunk, expanded)) = stack.pop() {
            if found.iter().any(|seen| Arc::ptr_eq(seen, &chunk)) {
                continue;
            }
            if expanded {
                found.push(chunk);
                continue;
            }
            stack.push((chunk.clone(), true));
            for import in chunk.imports.iter().rev() {
                stack.push((import.clone(), false));
            }
        }
        found
    }
}

impl Drop for Chunk<'_> {
    /// Dropping a chunk drops the chunks only it held, and theirs, however long the chain, in a
    /// loop rather than a recursion.
    fn drop(&mut self) {
        let mut held = std::mem::take(&mut self.imports);
        while let Some(import) = held.pop() {
            if let Some(mut chunk) = Arc::into_inner(import) {
                held.append(&mut chunk.imports);
            }
        }
    }
}

impl std::fmt::Debug for Chunk<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Chunk({:016x}, {} nodes)", self.id, self.counts[NODES])
    }
}
