//! Changes made in place, to a chunk only one document holds.

use std::borrow::Cow;
use std::sync::Arc;

use super::{Chunk, EXTERN, IMPORTS, KIDS, NODES, corrupt};

/// The most imports a change in place gives a chunk, past which the change copies instead.
const IMPORTS_IN_PLACE: usize = 16;

impl<'a> Chunk<'a> {
    /// Puts node `index` of `holder`, `size` positions long, in place of kid `position` of node
    /// `id`'s list of kids, and changes the node's size to match: `false`, having changed
    /// nothing, unless this chunk is held by nothing else and owns its bytes, and the list is
    /// in it, the node being its last, or in an import held by nothing else that owns its bytes.
    pub(crate) fn replace_kid_in_place(
        this: &mut Arc<Chunk<'a>>,
        id: u32,
        position: u32,
        holder: &Arc<Chunk<'a>>,
        index: u32,
        size: u32,
    ) -> bool {
        let Some(chunk) = Arc::get_mut(this).filter(|chunk| chunk.is_owned()) else {
            return false;
        };
        let record = chunk.record(id);
        if position >= record.b {
            return false;
        }
        let new_size = if record.a & EXTERN == 0 {
            // A list in this chunk only for its last node, which no node in it lists, and whose
            // list no other node in it lists either, each list being written for the node
            // after it.
            if id.checked_add(1) != Some(chunk.counts[NODES]) {
                return false;
            }
            let at = record.a.checked_add(position).unwrap_or_else(|| corrupt());
            chunk.put_kid(at, record.size, holder, index, size)
        } else {
            let (slot, start) = chunk.external(record.a);
            let at = start.checked_add(position).unwrap_or_else(|| corrupt());
            let Some(list) = (chunk.imports.get_mut(slot as usize))
                .and_then(Arc::get_mut)
                .filter(|list| list.is_owned())
            else {
                return false;
            };
            list.put_kid(at, record.size, holder, index, size)
        };
        let Some(content) = new_size else {
            return false;
        };
        let node = chunk.at(NODES, id);
        chunk.write_words(node + 20, &[content]);
        true
    }

    /// Puts node `index` of `holder`, `size` positions long, as kid `at` of this chunk, which
    /// must own its bytes, importing `holder`: the size of content `content_size` long with the
    /// kid changed, or `None`, having changed nothing, where that or the import doesn't fit.
    fn put_kid(
        &mut self,
        at: u32,
        content_size: u32,
        holder: &Arc<Chunk<'a>>,
        index: u32,
        size: u32,
    ) -> Option<u32> {
        let content = u64::from(content_size) - u64::from(self.kid(at).size) + u64::from(size);
        let content = u32::try_from(content).ok()?;
        let slot = self.import_in_place(holder)?;
        let kid = self.at(KIDS, at);
        self.write_words(kid, &[slot, index, size]);
        Some(content)
    }

    fn is_owned(&self) -> bool {
        matches!(self.bytes, Cow::Owned(_))
    }

    fn owned_bytes(&mut self) -> &mut Vec<u8> {
        match &mut self.bytes {
            Cow::Owned(bytes) => bytes,
            Cow::Borrowed(_) => panic!("a chunk changed in place owns its bytes"),
        }
    }

    /// Words of the chunk's own bytes written over, at byte `at`.
    fn write_words(&mut self, at: usize, words: &[u32]) {
        let bytes = &mut self.owned_bytes()[at..];
        for (word, bytes) in words.iter().zip(bytes.as_chunks_mut::<4>().0) {
            *bytes = word.to_le_bytes();
        }
    }

    /// The slot of `chunk`, importing it, at the end, when it isn't an import yet: `None` once
    /// the chunk has as many imports as changes in place may give it, which would otherwise
    /// keep every chunk they made alive.
    fn import_in_place(&mut self, chunk: &Arc<Chunk<'a>>) -> Option<u32> {
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
        self.owned_bytes()
            .extend_from_slice(&chunk.id.to_le_bytes());
        let slot = self.imports.len() as u32;
        self.counts[IMPORTS] = slot + 1;
        self.write_words(24 + IMPORTS * 4, &[slot + 1]);
        self.imports.push(chunk.clone());
        Some(slot)
    }
}
