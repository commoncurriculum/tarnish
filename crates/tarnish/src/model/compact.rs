//! Writing a node again into a chunk of its own: whole, or all but what some chunks hold.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::node::{Change, Node};
use super::schema::Schema;
use super::view::{NodeRef, SetRef};
use crate::chunk::{
    Builder, Chunk, EMPTY_OBJECT, EMPTY_SET, Holder, Kid, LOCAL, Record, ValueRef, corrupt,
};
use crate::js::stack;

/// What a copy has written of what it copies: each value and set it copied, by where it was,
/// so that what nodes share stays shared.
struct Copier<'k, 'a> {
    builder: Builder<'a>,
    values: FxHashMap<(usize, u32), u32>,
    sets: FxHashMap<(usize, u32), u32>,
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
            values: FxHashMap::default(),
            sets: FxHashMap::default(),
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
        self.element(node, marks, None, record.size)
    }

    /// Writes the record of `node`, `size` positions long, and its list of kids, each kid's node
    /// copied unless a chunk kept holds it, and kid `at` replaced by `kid` when `substitute` is
    /// `(at, kid)`: its index.
    fn element(
        &mut self,
        node: NodeRef,
        marks: u32,
        substitute: Option<(u32, Kid)>,
        size: u32,
    ) -> u32 {
        let record = node.record;
        let (list, start, bound) = node.kids();
        let count = node.child_count();
        // Each of the first slots, and the list's own, names a chunk kept, whose slot here is
        // found once, or one not kept, whose kids' nodes are copied first. The list is then
        // copied as it is, with those slots rewritten, and the other kids written over it.
        const SLOTS: usize = 16;
        const UNSEEN: u32 = u32::MAX - 1;
        let mut slots = [UNSEEN; SLOTS + 1];
        let mut written = Vec::new();
        for (position, kid) in (0..count).zip(list.kid_list(start, count)) {
            if let Some((at, kid)) = substitute
                && at == position
            {
                written.push((position, kid));
                continue;
            }
            let at = match kid.slot {
                LOCAL if kid.index >= bound => corrupt(),
                LOCAL => Some(SLOTS),
                slot => Some(slot as usize).filter(|&slot| slot < SLOTS),
            };
            let slot = match at.map(|at| slots[at]) {
                Some(slot) if slot != UNSEEN => slot,
                _ => {
                    let slot = match self.kept_chunk(list.holder(kid, bound)) {
                        Some(kept) => self.builder.import(kept),
                        None => LOCAL,
                    };
                    if let Some(at) = at {
                        slots[at] = slot;
                    }
                    slot
                }
            };
            if slot == LOCAL {
                let node = NodeRef::at(list.holder(kid, bound), kid.index);
                let index = stack::grow(|| self.copy(node));
                written.push((position, Kid::local(index, kid.size)));
            } else if at.is_none() {
                written.push((position, Kid { slot, ..kid }));
            }
        }
        let attrs = self.value(node.attrs());
        let first = self
            .builder
            .copy_kid_list(list, start, count, &slots, &written);
        self.builder
            .element(record.ty, marks, attrs, first, count, size)
    }

    /// [`copy`](Self::copy) of `node` with the descendant `path` leads to replaced by `bottom`:
    /// its index, and its size.
    fn copy_path(&mut self, node: NodeRef, path: &[u32], bottom: NodeRef) -> (u32, u32) {
        let marks = self.set(node.marks());
        let (list, start, bound) = node.kids();
        let old = list
            .kid_list(start, node.child_count())
            .nth(path[0] as usize)
            .unwrap_or_else(|| corrupt());
        let (id, size) = match path {
            [_] => (stack::grow(|| self.copy(bottom)), bottom.node_size() as u32),
            [_, below @ ..] => {
                let child = NodeRef::at(list.holder(old, bound), old.index);
                stack::grow(|| self.copy_path(child, below, bottom))
            }
            [] => unreachable!("a path"),
        };
        let content = node.record.size - old.size + size;
        let substitute = Some((path[0], Kid::local(id, size)));
        (self.element(node, marks, substitute, content), content + 2)
    }
}

impl<'a> Change<'a> {
    /// The changed document written into a new chunk, as [`Node::flatten`] writes it, the nodes
    /// along the path written there and nowhere else.
    pub fn flatten(&self, kept: &[Arc<Chunk<'a>>]) -> Node<'a> {
        if self.path.is_empty() {
            return self.bottom.flatten(kept);
        }
        let mut copier = Copier::new(self.doc.schema(), kept);
        let (id, _) = copier.copy_path(self.doc.view(), &self.path, self.bottom.view());
        Node::at(copier.builder.seal(), id)
    }

    /// The chunks the change wrote, the document's and the bottom's, and the bytes the nodes
    /// along the path take once written: a record and a list each.
    pub fn written(&self) -> ([&Arc<Chunk<'a>>; 2], usize) {
        let mut bytes = 0;
        let mut node = self.doc.view();
        for &index in &self.path {
            bytes += 24 + 12 * node.child_count() as usize;
            node = node.child(index);
        }
        ([self.doc.chunk(), self.bottom.chunk()], bytes)
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
