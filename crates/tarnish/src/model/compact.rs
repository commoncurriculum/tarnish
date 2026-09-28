//! Writing a node again into a chunk of its own: whole, or all but what some chunks hold.

use std::collections::HashMap;
use std::sync::Arc;

use super::node::Node;
use super::schema::Schema;
use super::view::{NodeRef, SetRef};
use crate::chunk::{Builder, Chunk, EMPTY_OBJECT, EMPTY_SET, Holder, Kid, Record, ValueRef};
use crate::stack;

/// What a copy has written of what it copies: each value and set it copied, by where it was,
/// so that what nodes share stays shared.
struct Copier<'k, 'a> {
    builder: Builder<'a>,
    values: HashMap<(usize, u32), u32>,
    sets: HashMap<(usize, u32), u32>,
    /// Chunks whose nodes, values, sets and text are referred to rather than copied.
    kept: &'k [Arc<Chunk<'a>>],
}

fn address(chunk: &Chunk) -> usize {
    chunk.bytes().as_ptr() as usize
}

impl<'k, 'a> Copier<'k, 'a> {
    fn new(schema: &Schema, kept: &'k [Arc<Chunk<'a>>]) -> Copier<'k, 'a> {
        Copier {
            builder: Builder::new(schema),
            values: HashMap::new(),
            sets: HashMap::new(),
            kept,
        }
    }

    fn kept_chunk(&self, chunk: &Chunk) -> Option<&'k Arc<Chunk<'a>>> {
        self.kept.iter().find(|kept| kept.ptr_eq(chunk))
    }

    /// A ref to element `index` of `chunk`, when that is a chunk kept.
    fn kept(&mut self, chunk: &Chunk, index: u32) -> Option<u32> {
        let kept = self.kept_chunk(chunk)?;
        Some(self.builder.external(kept, index))
    }

    fn value(&mut self, value: ValueRef) -> u32 {
        if value.index == EMPTY_OBJECT {
            return EMPTY_OBJECT;
        }
        let key = (address(value.chunk), value.index);
        if let Some(&written) = self.values.get(&key) {
            return written;
        }
        let written = match self.kept(value.chunk, value.index) {
            Some(kept) => kept,
            None => self.builder.write(value),
        };
        self.values.insert(key, written);
        written
    }

    fn set(&mut self, set: SetRef) -> u32 {
        if set.is_empty() {
            return EMPTY_SET;
        }
        let key = (address(set.chunk), set.set);
        if let Some(&written) = self.sets.get(&key) {
            return written;
        }
        let written = match self.kept(set.chunk, set.set) {
            Some(kept) => kept,
            None => {
                let members: Vec<u32> = set
                    .iter()
                    .map(|mark| match self.kept(mark.chunk, mark.index) {
                        Some(kept) => kept,
                        None => {
                            let attrs = self.value(mark.attrs());
                            self.builder.mark(mark.rank() as u32, attrs)
                        }
                    })
                    .collect();
                self.builder.set(&members)
            }
        };
        self.sets.insert(key, written);
        written
    }

    /// The node written into the chunk: its index.
    fn copy(&mut self, node: NodeRef) -> u32 {
        let marks = self.set(node.marks());
        let record = node.record;
        if let Some(text) = node.text() {
            let (chunk, start) = node.chunk.resolve(record.a);
            return match self.kept(chunk, start) {
                Some(a) => self.builder.text_record(Record {
                    marks,
                    attrs: EMPTY_OBJECT,
                    a,
                    ..record
                }),
                None => self.builder.text_of(record.ty, marks, &text.to_text()),
            };
        }
        let (list, start, bound) = node.kids();
        let count = node.child_count();
        let mut kids = Vec::with_capacity(count as usize);
        // Kids come in runs from one slot, whose slot here, if its chunk is kept, is found once
        // a run.
        let mut run: Option<(u32, Option<u32>)> = None;
        for kid in list.kid_list(start, count) {
            let chunk = list.holder(kid, bound);
            let slot = match run {
                Some((last, slot)) if last == kid.slot => slot,
                _ => {
                    let slot = self.kept_chunk(chunk).map(|kept| self.builder.import(kept));
                    run = Some((kid.slot, slot));
                    slot
                }
            };
            kids.push(match slot {
                Some(slot) => Kid { slot, ..kid },
                None => {
                    let node = NodeRef::at(chunk, kid.index);
                    Kid::local(stack::grow(|| self.copy(node)), kid.size)
                }
            });
        }
        let attrs = self.value(node.attrs());
        let first = self.builder.push_kids(kids.into_iter());
        self.builder
            .element(record.ty, marks, attrs, first, count, record.size)
    }
}

impl Node<'_> {
    /// The node written again, all of it, into one chunk of its own: a document that shares
    /// nothing with others any more, and holds nothing it doesn't use.
    pub fn compact(&self) -> Node<'static> {
        write_again(self.view(), &[])
    }
}

impl<'a> Node<'a> {
    /// The node written again, as the last node of a new chunk, with all it holds that isn't
    /// in the chunks `kept`, to which it refers for the rest. A step's changes, made in many
    /// small chunks, go into one this way.
    pub fn flatten(&self, kept: &[Arc<Chunk<'a>>]) -> Node<'a> {
        write_again(self.view(), kept)
    }
}

fn write_again<'a>(node: NodeRef, kept: &[Arc<Chunk<'a>>]) -> Node<'a> {
    let mut copier = Copier::new(node.chunk.schema(), kept);
    let id = copier.copy(node);
    Node::at(copier.builder.seal(), id)
}
