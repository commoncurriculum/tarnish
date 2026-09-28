//! Writing a chunk: sections grow as nodes, values and marks are added, children before their
//! parents, and sealing packs them behind a header.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::BuildHasher;
use std::sync::Arc;

use super::value::{Kind, Tag, ValueRef};
use super::{
    ASCII, BINDING, Chunk, ELEMENTS, ENTRIES, EXTERN, EXTERNS, HEADER, HELD_AS_UNITS, IMPORTS,
    KIDS, Kid, LOCAL, MAGIC, MARKS, MEMBERS, NODES, Record, SECTIONS, SETS, STRINGS, TEXT,
    TEXT_NODE, UNITS, VALUES, corrupt,
};
use crate::json::Value;
use crate::model::{Schema, TextRef};
use crate::stack;
use crate::text::Text;

/// A chunk being written.
pub(crate) struct Builder<'a> {
    schema: Schema,
    imports: Vec<Arc<Chunk<'a>>>,
    counts: [u32; SECTIONS],
    scratch: Box<Scratch>,
}

/// What a builder writes into until it seals the chunk, which the next builder on the thread
/// reuses rather than allocating and growing its own.
#[derive(Default)]
struct Scratch {
    sections: [Vec<u8>; SECTIONS],
    /// Each import's slot, by the address of its chunk, once there are too many to search.
    import_slots: HashMap<usize, u32>,
    /// Keys written to `strings`, to write each once.
    keys: Vec<(u32, u32)>,
    /// Each node type's default attributes, once written.
    defaults: Vec<Option<u32>>,
    /// Each mark type's mark with its defaults, and the set of just that mark, once written.
    instances: Vec<Option<(u32, u32)>>,
    /// Items of the arrays and objects being written, the innermost's last.
    items: Vec<u32>,
    entries: Vec<(u32, u32, u32)>,
}

/// Keys searched for before they are written again, up to this many.
const KEYS: usize = 64;

thread_local! {
    static SPARE: RefCell<Vec<Box<Scratch>>> = const { RefCell::new(Vec::new()) };
}

/// Scratches a thread keeps, and the most bytes a kept section holds on to.
const SPARES: usize = 8;
const SPARE_BYTES: usize = 1 << 16;

fn scratch() -> Box<Scratch> {
    let spare = SPARE.try_with(|spare| spare.try_borrow_mut().ok()?.pop());
    spare.ok().flatten().unwrap_or_default()
}

fn recycle(mut scratch: Box<Scratch>) {
    for section in &mut scratch.sections {
        section.clear();
        if section.capacity() > SPARE_BYTES {
            *section = Vec::new();
        }
    }
    scratch.import_slots.clear();
    scratch.keys.clear();
    scratch.defaults.clear();
    scratch.instances.clear();
    scratch.items.clear();
    scratch.entries.clear();
    let _ = SPARE.try_with(|spare| {
        if let Ok(mut spare) = spare.try_borrow_mut()
            && spare.len() < SPARES
        {
            spare.push(scratch);
        }
    });
}

impl<'a> Builder<'a> {
    pub fn new(schema: &Schema) -> Builder<'a> {
        let mut builder = Builder {
            schema: schema.clone(),
            imports: Vec::new(),
            counts: [0; SECTIONS],
            scratch: scratch(),
        };
        builder.push_words(SETS, &[0, 0]);
        builder.push_value(Tag::Object, 0, 0);
        builder
    }

    fn push(&mut self, section: usize, bytes: &[u8]) -> u32 {
        let index = self.counts[section];
        self.scratch.sections[section].extend_from_slice(bytes);
        let added = (bytes.len() / super::WIDTHS[section]) as u32;
        self.counts[section] = index
            .checked_add(added)
            .filter(|&count| count < EXTERN)
            .expect("a chunk holds fewer than 2^31 of anything");
        index
    }

    fn push_words(&mut self, section: usize, words: &[u32]) -> u32 {
        let mut bytes = [0; 12];
        for (at, word) in words.iter().enumerate() {
            bytes[at * 4..at * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        self.push(section, &bytes[..words.len() * 4])
    }

    fn push_value(&mut self, tag: Tag, a: u32, b: u32) -> u32 {
        self.push_words(VALUES, &[tag as u32, a, b])
    }

    /// The slot of an imported chunk, importing it if it isn't yet.
    pub fn import(&mut self, chunk: &Arc<Chunk<'a>>) -> u32 {
        let address = Arc::as_ptr(chunk) as *const u8 as usize;
        if let Some(last) = self.imports.last()
            && Arc::ptr_eq(last, chunk)
        {
            return self.imports.len() as u32 - 1;
        }
        if self.imports.len() <= 8 {
            if let Some(slot) = self
                .imports
                .iter()
                .position(|seen| Arc::ptr_eq(seen, chunk))
            {
                return slot as u32;
            }
        } else if let Some(&slot) = self.scratch.import_slots.get(&address) {
            return slot;
        }
        let slot = self.imports.len() as u32;
        self.imports.push(chunk.clone());
        if self.imports.len() > 8 {
            if self.scratch.import_slots.is_empty() {
                for (slot, chunk) in self.imports.iter().enumerate() {
                    self.scratch
                        .import_slots
                        .insert(Arc::as_ptr(chunk) as *const u8 as usize, slot as u32);
                }
            }
            self.scratch.import_slots.insert(address, slot);
        }
        slot
    }

    /// A ref to index `index` of another chunk.
    pub fn external(&mut self, chunk: &Arc<Chunk<'a>>, index: u32) -> u32 {
        let slot = self.import(chunk);
        EXTERN | self.push_words(EXTERNS, &[slot, index])
    }

    /// A ref, in this chunk, to what `reference` names in `chunk`.
    pub fn reference(&mut self, chunk: &Arc<Chunk<'a>>, reference: u32) -> u32 {
        let (chunk, index) = Chunk::resolve_shared(chunk, reference);
        self.external(chunk, index)
    }

    /// [`reference`](Self::reference) to a set or value: set and value 0, empty in every chunk,
    /// stay 0.
    pub fn reference_markup(&mut self, chunk: &Arc<Chunk<'a>>, reference: u32) -> u32 {
        match reference {
            0 => 0,
            reference => self.reference(chunk, reference),
        }
    }

    /// Adds to the kids section kids `from..to` of the list at `start` in `chunk`, which must be
    /// below `bound` where they are in `chunk`.
    pub fn copy_kids(
        &mut self,
        chunk: &Arc<Chunk<'a>>,
        start: u32,
        bound: u32,
        from: u32,
        to: u32,
    ) {
        let count = to.saturating_sub(from);
        let kids = chunk.span(
            KIDS,
            start.checked_add(from).unwrap_or_else(|| corrupt()),
            count,
        );
        self.counts[KIDS] = self.counts[KIDS]
            .checked_add(count)
            .filter(|&total| total < EXTERN)
            .expect("a chunk holds fewer than 2^31 of anything");
        let mut list = std::mem::take(&mut self.scratch.sections[KIDS]);
        let written = list.len();
        list.resize(written + kids.len(), 0);
        let copies = list[written..].as_chunks_mut::<12>().0;
        // `chunk`'s own slot here, and its first import slots, once used.
        let mut own = LOCAL;
        let mut slots = [LOCAL; 8];
        for (kid, copy) in kids.as_chunks::<12>().0.iter().zip(copies) {
            let theirs = u32::from_le_bytes([kid[0], kid[1], kid[2], kid[3]]);
            let index = u32::from_le_bytes([kid[4], kid[5], kid[6], kid[7]]);
            let slot = match theirs {
                LOCAL if index >= bound => corrupt(),
                LOCAL if own != LOCAL => own,
                LOCAL => {
                    own = self.import(chunk);
                    own
                }
                theirs => {
                    let import = chunk.import(theirs);
                    match slots.get_mut(theirs as usize) {
                        Some(slot) if *slot != LOCAL => *slot,
                        Some(slot) => {
                            *slot = self.import(import);
                            *slot
                        }
                        None => self.import(import),
                    }
                }
            };
            copy[..4].copy_from_slice(&slot.to_le_bytes());
            copy[4..].copy_from_slice(&kid[4..]);
        }
        self.scratch.sections[KIDS] = list;
    }

    /// A kid for node `index` of `chunk`, which is `size` positions long.
    pub fn kid(&mut self, chunk: &Arc<Chunk<'a>>, index: u32, size: usize) -> Kid {
        Kid {
            slot: self.import(chunk),
            index,
            size: u32::try_from(size).expect("a node smaller than 4G positions"),
        }
    }

    /// Adds a kid to the kids section.
    pub fn push_kid(&mut self, kid: Kid) {
        self.push(KIDS, &kid.to_bytes());
    }

    /// Where a string is in `strings`.
    pub fn string(&mut self, string: &str) -> (u32, u32) {
        let len = u32::try_from(string.len()).expect("a string shorter than 4 GiB");
        (self.push(STRINGS, string.as_bytes()), len)
    }

    fn key(&mut self, key: &str) -> (u32, u32) {
        let strings = &self.scratch.sections[STRINGS];
        let found = self.scratch.keys.iter().find(|&&(start, len)| {
            len as usize == key.len()
                && &strings[start as usize..start as usize + len as usize] == key.as_bytes()
        });
        if let Some(&found) = found {
            return found;
        }
        let written = self.string(key);
        if self.scratch.keys.len() < KEYS {
            self.scratch.keys.push(written);
        }
        written
    }

    pub fn value(&mut self, value: &Value) -> u32 {
        match value {
            Value::Null => self.push_value(Tag::Null, 0, 0),
            Value::Bool(false) => self.push_value(Tag::False, 0, 0),
            Value::Bool(true) => self.push_value(Tag::True, 0, 0),
            Value::Number(number) => self.number(number),
            Value::String(string) => {
                let (start, len) = self.string(string);
                self.push_value(Tag::String, start, len)
            }
            Value::Array(items) => {
                let base = self.scratch.items.len();
                for item in items {
                    let item = stack::grow(|| self.value(item));
                    self.scratch.items.push(item);
                }
                self.array(base)
            }
            Value::Object(map) => {
                let base = self.scratch.entries.len();
                for (key, value) in map {
                    let value = stack::grow(|| self.value(value));
                    let (start, len) = self.key(key);
                    self.scratch.entries.push((start, len, value));
                }
                self.object(base)
            }
        }
    }

    /// An object's value; value 0, the empty object, for an empty one.
    pub fn map(&mut self, map: &crate::json::Map) -> u32 {
        if map.is_empty() {
            return 0;
        }
        let base = self.scratch.entries.len();
        for (key, value) in map {
            let value = stack::grow(|| self.value(value));
            let (start, len) = self.key(key);
            self.scratch.entries.push((start, len, value));
        }
        self.object(base)
    }

    fn number(&mut self, number: &crate::json::Number) -> u32 {
        let (tag, bits) = if let Some(integer) = number.as_i64() {
            (Tag::Int, integer as u64)
        } else if let Some(integer) = number.as_u64() {
            (Tag::UInt, integer)
        } else {
            (Tag::Float, number.as_f64().unwrap_or_default().to_bits())
        };
        self.push_value(tag, bits as u32, (bits >> 32) as u32)
    }

    fn array(&mut self, base: usize) -> u32 {
        let start = self.counts[ELEMENTS];
        let len = (self.scratch.items.len() - base) as u32;
        let mut bytes = std::mem::take(&mut self.scratch.sections[ELEMENTS]);
        for item in self.scratch.items.drain(base..) {
            bytes.extend_from_slice(&item.to_le_bytes());
        }
        self.scratch.sections[ELEMENTS] = bytes;
        self.counts[ELEMENTS] += len;
        self.push_value(Tag::Array, start, len)
    }

    fn object(&mut self, base: usize) -> u32 {
        let start = self.counts[ENTRIES];
        let len = (self.scratch.entries.len() - base) as u32;
        let mut bytes = std::mem::take(&mut self.scratch.sections[ENTRIES]);
        for (key, key_len, value) in self.scratch.entries.drain(base..) {
            for word in [key, key_len, value] {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
        }
        self.scratch.sections[ENTRIES] = bytes;
        self.counts[ENTRIES] += len;
        self.push_value(Tag::Object, start, len)
    }

    /// An object of these entries, each a key and a value written before.
    pub fn object_of<'k>(&mut self, entries: impl IntoIterator<Item = (&'k str, u32)>) -> u32 {
        let base = self.scratch.entries.len();
        for (key, value) in entries {
            let (start, len) = self.key(key);
            self.scratch.entries.push((start, len, value));
        }
        self.object(base)
    }

    /// A value in another chunk, copied into this one.
    pub fn copy_value(&mut self, value: ValueRef) -> u32 {
        match value.kind() {
            Kind::Null => self.push_value(Tag::Null, 0, 0),
            Kind::Bool(false) => self.push_value(Tag::False, 0, 0),
            Kind::Bool(true) => self.push_value(Tag::True, 0, 0),
            Kind::Number(number) => self.number(&number),
            Kind::String(string) => {
                let (start, len) = self.string(string);
                self.push_value(Tag::String, start, len)
            }
            Kind::Array(..) => {
                let base = self.scratch.items.len();
                for item in value.items() {
                    let item = stack::grow(|| self.copy_value(item));
                    self.scratch.items.push(item);
                }
                self.array(base)
            }
            Kind::Object(..) => {
                let base = self.scratch.entries.len();
                for (key, item) in value.entries() {
                    let item = stack::grow(|| self.copy_value(item));
                    let (start, len) = self.key(key);
                    self.scratch.entries.push((start, len, item));
                }
                self.object(base)
            }
        }
    }

    /// The value of a node type's default attributes, written once.
    pub fn defaults(&mut self, type_index: usize, defaults: &crate::json::Map) -> u32 {
        if defaults.is_empty() {
            return 0;
        }
        if self.scratch.defaults.len() <= type_index {
            self.scratch.defaults.resize(type_index + 1, None);
        }
        if let Some(written) = self.scratch.defaults[type_index] {
            return written;
        }
        let base = self.scratch.entries.len();
        for (key, value) in defaults {
            let value = self.value(value);
            let (start, len) = self.key(key);
            self.scratch.entries.push((start, len, value));
        }
        let written = self.object(base);
        self.scratch.defaults[type_index] = Some(written);
        written
    }

    /// Word `word` of element `index` of a section written so far.
    fn written(&self, section: usize, index: u32, word: usize) -> u32 {
        let at = index as usize * super::WIDTHS[section] + word * 4;
        let bytes = &self.scratch.sections[section][at..at + 4];
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    fn written_string(&self, start: u32, len: u32) -> &[u8] {
        &self.scratch.sections[STRINGS][start as usize..(start + len) as usize]
    }

    /// Whether two sets written here hold equal marks, as `Mark.sameSet` compares them.
    pub fn sets_equal(&self, a: u32, b: u32) -> bool {
        if a == b {
            return true;
        }
        let (a_start, a_len) = (self.written(SETS, a, 0), self.written(SETS, a, 1));
        let (b_start, b_len) = (self.written(SETS, b, 0), self.written(SETS, b, 1));
        a_len == b_len
            && (0..a_len).all(|at| {
                let (a, b) = (
                    self.written(MEMBERS, a_start + at, 0),
                    self.written(MEMBERS, b_start + at, 0),
                );
                a == b
                    || (a & EXTERN == 0
                        && b & EXTERN == 0
                        && self.written(MARKS, a, 0) == self.written(MARKS, b, 0)
                        && self.values_equal(self.written(MARKS, a, 1), self.written(MARKS, b, 1)))
            })
    }

    /// `compareDeep` of two values written here.
    fn values_equal(&self, a: u32, b: u32) -> bool {
        if a == b {
            return true;
        }
        if a & EXTERN != 0 || b & EXTERN != 0 {
            return false;
        }
        let tag = |value| Tag::from_word(self.written(VALUES, value, 0));
        let words = |value| {
            (
                self.written(VALUES, value, 1),
                self.written(VALUES, value, 2),
            )
        };
        let number = |value| {
            let (low, high) = words(value);
            let bits = u64::from(low) | u64::from(high) << 32;
            match tag(value) {
                Tag::Int => bits as i64 as f64,
                Tag::UInt => bits as f64,
                _ => f64::from_bits(bits),
            }
        };
        let ((a_start, a_len), (b_start, b_len)) = (words(a), words(b));
        match (tag(a), tag(b)) {
            (Tag::Int | Tag::UInt | Tag::Float, Tag::Int | Tag::UInt | Tag::Float) => {
                number(a) == number(b)
            }
            (Tag::String, Tag::String) => {
                self.written_string(a_start, a_len) == self.written_string(b_start, b_len)
            }
            (Tag::Array, Tag::Array) => {
                a_len == b_len
                    && (0..a_len).all(|at| {
                        let (a, b) = (
                            self.written(ELEMENTS, a_start + at, 0),
                            self.written(ELEMENTS, b_start + at, 0),
                        );
                        stack::grow(|| self.values_equal(a, b))
                    })
            }
            (Tag::Object, Tag::Object) => {
                a_len == b_len
                    && (0..a_len).all(|at| {
                        let key = self.written_string(
                            self.written(ENTRIES, a_start + at, 0),
                            self.written(ENTRIES, a_start + at, 1),
                        );
                        let value = self.written(ENTRIES, a_start + at, 2);
                        (0..b_len).any(|other| {
                            let other_key = self.written_string(
                                self.written(ENTRIES, b_start + other, 0),
                                self.written(ENTRIES, b_start + other, 1),
                            );
                            other_key == key
                                && stack::grow(|| {
                                    self.values_equal(
                                        value,
                                        self.written(ENTRIES, b_start + other, 2),
                                    )
                                })
                        })
                    })
            }
            (a, b) => a == b,
        }
    }

    /// The text of a text node written here.
    pub fn text_of_record(&self, record: Record) -> Text {
        if record.flags & HELD_AS_UNITS != 0 {
            let bytes = &self.scratch.sections[UNITS]
                [record.a as usize * 2..(record.a + record.b) as usize * 2];
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&pair| u16::from_le_bytes(pair))
                .collect();
            return Text::from_units(&units);
        }
        let bytes = &self.scratch.sections[TEXT][record.a as usize..(record.a + record.b) as usize];
        Text::from(std::str::from_utf8(bytes).expect("text the builder wrote"))
    }

    /// A mark type's instance and the set of just it, when written.
    pub fn instance_of(&self, rank: usize) -> Option<(u32, u32)> {
        self.scratch.instances.get(rank).copied().flatten()
    }

    pub fn mark(&mut self, rank: u32, attrs: u32) -> u32 {
        self.push_words(MARKS, &[rank, attrs])
    }

    /// A mark type's mark with its defaults, and the set of just that mark, written once.
    pub fn instance(&mut self, rank: usize, defaults: &crate::json::Map) -> (u32, u32) {
        if self.scratch.instances.len() <= rank {
            self.scratch.instances.resize(rank + 1, None);
        }
        if let Some(written) = self.scratch.instances[rank] {
            return written;
        }
        let attrs = if defaults.is_empty() {
            0
        } else {
            let base = self.scratch.entries.len();
            for (key, value) in defaults {
                let value = self.value(value);
                let (start, len) = self.key(key);
                self.scratch.entries.push((start, len, value));
            }
            self.object(base)
        };
        let mark = self.mark(rank as u32, attrs);
        let set = self.set(&[mark]);
        self.scratch.instances[rank] = Some((mark, set));
        (mark, set)
    }

    /// A set of these marks, each a mark ref.
    pub fn set(&mut self, members: &[u32]) -> u32 {
        if members.is_empty() {
            return 0;
        }
        let start = self.counts[MEMBERS];
        for &member in members {
            self.push_words(MEMBERS, &[member]);
        }
        self.push_words(SETS, &[start, members.len() as u32])
    }

    pub fn record(&self, id: u32) -> Record {
        let at = id as usize * 24;
        Record::from_bytes_of(&self.scratch.sections[NODES][at..at + 24])
    }

    fn push_record(&mut self, record: Record) -> u32 {
        self.push(NODES, &record.to_bytes())
    }

    pub fn set_record(&mut self, id: u32, record: Record) {
        let at = id as usize * 24;
        self.scratch.sections[NODES][at..at + 24].copy_from_slice(&record.to_bytes());
    }

    /// Sets a flag of node `id`.
    pub fn flag(&mut self, id: u32, flag: u16) {
        let mut record = self.record(id);
        record.flags |= flag;
        self.set_record(id, record);
    }

    /// Take back the last node written.
    pub fn pop_node(&mut self) {
        let len = self.scratch.sections[NODES].len() - 24;
        self.scratch.sections[NODES].truncate(len);
        self.counts[NODES] -= 1;
    }

    /// Where the text section ends.
    pub fn text_end(&self) -> u32 {
        self.counts[TEXT]
    }

    pub fn text(&mut self, ty: u16, marks: u32, text: &str) -> u32 {
        let (flags, size) = if text.is_ascii() {
            (TEXT_NODE | ASCII, text.len())
        } else {
            (TEXT_NODE, text.chars().map(char::len_utf16).sum())
        };
        let a = self.push(TEXT, text.as_bytes());
        self.push_record(Record {
            ty,
            flags,
            marks,
            attrs: 0,
            a,
            b: text.len() as u32,
            size: u32::try_from(size).expect("a text shorter than 4G units"),
        })
    }

    /// Text that holds a lone surrogate.
    pub fn text_units(&mut self, ty: u16, marks: u32, units: &[u16]) -> u32 {
        let bytes: Vec<u8> = units.iter().flat_map(|unit| unit.to_le_bytes()).collect();
        let a = self.push(UNITS, &bytes);
        self.push_record(Record {
            ty,
            flags: TEXT_NODE | HELD_AS_UNITS,
            marks,
            attrs: 0,
            a,
            b: units.len() as u32,
            size: units.len() as u32,
        })
    }

    pub fn text_of(&mut self, ty: u16, marks: u32, text: &Text) -> u32 {
        match text.as_str() {
            Some(string) => self.text(ty, marks, string),
            None => self.text_units(ty, marks, &text.units()),
        }
    }

    /// A text node of two texts, one after the other.
    pub fn text_of_parts(&mut self, ty: u16, marks: u32, first: TextRef, second: TextRef) -> u32 {
        let (
            TextRef::Utf8 {
                text: a,
                ascii: a_ascii,
                ..
            },
            TextRef::Utf8 {
                text: b,
                ascii: b_ascii,
                ..
            },
        ) = (first, second)
        else {
            let text: Text = [first.to_text(), second.to_text()].iter().collect();
            return self.text_of(ty, marks, &text);
        };
        let start = self.push(TEXT, a.as_bytes());
        self.push(TEXT, b.as_bytes());
        let flags = match a_ascii && b_ascii {
            true => TEXT_NODE | ASCII,
            false => TEXT_NODE,
        };
        self.push_record(Record {
            ty,
            flags,
            marks,
            attrs: 0,
            a: start,
            b: (a.len() + b.len()) as u32,
            size: (first.len() + second.len()) as u32,
        })
    }

    /// A text node whose text is in another chunk, `record`'s text ref being a ref here.
    pub fn text_record(&mut self, record: Record) -> u32 {
        self.push_record(Record {
            flags: record.flags & !BINDING,
            ..record
        })
    }

    /// A node that isn't text, with these kids and content this size.
    pub fn element(
        &mut self,
        ty: u16,
        marks: u32,
        attrs: u32,
        kids: impl ExactSizeIterator<Item = Kid>,
        size: u32,
    ) -> u32 {
        let count = kids.len() as u32;
        let first = self.kids(kids);
        self.element_of(ty, marks, attrs, first, count, size)
    }

    /// A node that isn't text, whose `count` kids are listed from `first`, a kids ref.
    pub fn element_of(
        &mut self,
        ty: u16,
        marks: u32,
        attrs: u32,
        first: u32,
        count: u32,
        size: u32,
    ) -> u32 {
        self.push_record(Record {
            ty,
            flags: 0,
            marks,
            attrs,
            a: first,
            b: count,
            size,
        })
    }

    /// Where the next kid added to the kids section goes.
    pub fn kids_end(&self) -> u32 {
        self.counts[KIDS]
    }

    /// A list of kids: where it starts.
    pub fn kids(&mut self, kids: impl Iterator<Item = Kid>) -> u32 {
        let start = self.counts[KIDS];
        for kid in kids {
            self.push_kid(kid);
        }
        start
    }

    /// The chunk's bytes, a header and each section in turn, and where each section starts.
    fn pack(&self, id: u64) -> (Vec<u8>, [u32; SECTIONS], [u32; SECTIONS]) {
        let mut counts = self.counts;
        counts[IMPORTS] = self.imports.len() as u32;
        let mut header = [0; HEADER];
        header[..4].copy_from_slice(&MAGIC);
        header[8..16].copy_from_slice(&self.schema.fingerprint().to_le_bytes());
        header[16..24].copy_from_slice(&id.to_le_bytes());
        for (at, count) in counts.iter().enumerate() {
            header[24 + at * 4..28 + at * 4].copy_from_slice(&count.to_le_bytes());
        }
        let sections = &self.scratch.sections[..IMPORTS];
        let total = HEADER + sections.iter().map(Vec::len).sum::<usize>() + self.imports.len() * 8;
        let mut bytes = Vec::with_capacity(total);
        bytes.extend_from_slice(&header);
        let mut starts = [0; SECTIONS];
        for (section, written) in sections.iter().enumerate() {
            starts[section] = u32::try_from(bytes.len()).expect("a chunk smaller than 4 GiB");
            bytes.extend_from_slice(written);
        }
        starts[IMPORTS] = u32::try_from(bytes.len()).expect("a chunk smaller than 4 GiB");
        for import in &self.imports {
            bytes.extend_from_slice(&import.id().to_le_bytes());
        }
        (bytes, starts, counts)
    }

    pub fn seal(self) -> Arc<Chunk<'a>> {
        let (bytes, starts, counts) = self.pack(fresh_id());
        let Builder {
            schema,
            imports,
            scratch,
            ..
        } = self;
        recycle(scratch);
        Arc::new(Chunk::from_parts(bytes, schema, imports, starts, counts))
    }
}

impl Record {
    fn from_bytes_of(bytes: &[u8]) -> Record {
        let mut record = [0; 24];
        record.copy_from_slice(bytes);
        Record::from_bytes(&record)
    }
}

/// An id no other chunk is likely to have: each thread's own random sequence.
fn fresh_id() -> u64 {
    thread_local! {
        static NEXT: Cell<u64> = Cell::new(
            std::collections::hash_map::RandomState::new().hash_one(0x5eed_u64) | 1,
        );
    }
    NEXT.with(|next| {
        let mut id = next.get();
        id ^= id << 13;
        id ^= id >> 7;
        id ^= id << 17;
        next.set(id);
        id
    })
}
