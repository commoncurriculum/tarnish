//! Writing a node again, whole, into a chunk of its own.

use std::collections::HashMap;
use std::sync::Arc;

use super::node::Node;
use super::view::{NodeRef, SetRef};
use crate::chunk::{Builder, Chunk, ValueRef};
use crate::stack;

/// What a copy has written of what it copies: each value and set it copied, by where it was,
/// so that what nodes share stays shared.
struct Copier<'k, 'a> {
    builder: Builder<'a>,
    values: HashMap<(usize, u32), u32>,
    sets: HashMap<(usize, u32), u32>,
    /// Chunks whose nodes are referred to rather than copied.
    kept: &'k [Arc<Chunk<'a>>],
}

fn address(chunk: &Chunk) -> usize {
    chunk.bytes().as_ptr() as usize
}

impl<'a> Copier<'_, 'a> {
    fn value(&mut self, value: ValueRef) -> u32 {
        if value.index == 0 {
            return 0;
        }
        let key = (address(value.chunk), value.index);
        if let Some(&written) = self.values.get(&key) {
            return written;
        }
        let written = self.builder.copy_value(value);
        self.values.insert(key, written);
        written
    }

    fn set(&mut self, set: SetRef) -> u32 {
        if set.is_empty() {
            return 0;
        }
        let key = (address(set.chunk), set.set);
        if let Some(&written) = self.sets.get(&key) {
            return written;
        }
        let members: Vec<u32> = set
            .iter()
            .map(|mark| {
                let attrs = self.value(mark.attrs());
                self.builder.mark(mark.rank() as u32, attrs)
            })
            .collect();
        let written = self.builder.set(&members);
        self.sets.insert(key, written);
        written
    }

    /// The node written into the chunk, or referred to where it is in a chunk kept: a ref.
    fn node(&mut self, node: NodeRef) -> u32 {
        if let Some(kept) = self.kept.iter().find(|kept| kept.ptr_eq(node.chunk)) {
            let kept = kept.clone();
            return self.builder.external(&kept, node.id);
        }
        let marks = self.set(node.marks());
        if let Some(text) = node.text() {
            return self
                .builder
                .text_of(node.record.ty, marks, &text.to_text());
        }
        let kids: Vec<u32> = node
            .children()
            .map(|child| stack::grow(|| self.node(child)))
            .collect();
        let attrs = self.value(node.attrs());
        self.builder
            .element(node.record.ty, marks, attrs, &kids, node.record.size)
    }
}

impl Node<'_> {
    /// The node written again, all of it, into one chunk of its own: a document that shares
    /// nothing with others any more, and holds nothing it doesn't use.
    pub fn compact(&self) -> Node<'static> {
        let mut copier = Copier {
            builder: Builder::new(self.schema()),
            values: HashMap::new(),
            sets: HashMap::new(),
            kept: &[],
        };
        let id = copier.node(self.view());
        Node::at(copier.builder.seal(), id)
    }
}

impl<'a> Node<'a> {
    /// The node with all it holds outside the chunks `kept` written into one new chunk, which
    /// refers into those. A step's changes, made in many small chunks, go into one this way.
    pub fn flatten(&self, kept: &[Arc<Chunk<'a>>]) -> Node<'a> {
        if kept.iter().any(|kept| Arc::ptr_eq(kept, self.chunk())) {
            return self.clone();
        }
        let mut copier = Copier {
            builder: Builder::new(self.schema()),
            values: HashMap::new(),
            sets: HashMap::new(),
            kept,
        };
        let id = copier.node(self.view());
        Node::at(copier.builder.seal(), id)
    }
}
