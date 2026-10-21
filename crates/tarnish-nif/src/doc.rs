//! A document as Elixir holds it: `{schema, chunks}`, the chunks being binaries, newest first.
//! The newest chunk's last node is the document. The oldest is the chunk read from the JSON the
//! document keeps, whose maps its own JSON shares.
//!
//! A change writes one chunk more, which refers into the ones before it for what it didn't
//! change. That chunk takes in the newer chunks it outgrows, so that each chunk is more than
//! twice the size of the one after it, and there are few of them.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;

use rustler::{Binary, Encoder, Env, NewBinary, ResourceArc, Term};
use tarnish::chunk::{Chunk, Header};
use tarnish::{Node, NodeRef};

use crate::prosemirror::SchemaResource;

/// The most chunks a document is held in.
const MOST_CHUNKS: usize = 8;

/// The chunks a change wrote that are searched for one found again, past which they are kept
/// in a set.
const WRITTEN_SEARCHED: usize = 32;

pub struct Doc<'a> {
    /// The document's ref, `{schema, chunks}`.
    term: Term<'a>,
    schema: Term<'a>,
    /// The chunks' binaries, newest first.
    binaries: Vec<Term<'a>>,
    /// The chunks, oldest first.
    chunks: Vec<Arc<Chunk<'a>>>,
}

/// A chunk's bytes as a binary.
fn binary<'a>(env: Env<'a>, bytes: &[u8]) -> Term<'a> {
    let mut binary = NewBinary::new(env, bytes.len());
    binary.as_mut_slice().copy_from_slice(bytes);
    binary.into()
}

/// The ref of a document read from JSON, whose chunk is the node's own.
pub fn read<'a>(env: Env<'a>, schema: Term<'a>, doc: &Node) -> Term<'a> {
    (schema, [binary(env, doc.chunk().bytes())].as_slice()).encode(env)
}

impl<'a> Doc<'a> {
    /// The document a ref names; `None` for a term that isn't one.
    pub fn load(term: Term<'a>) -> Option<Doc<'a>> {
        let (schema, binaries): (Term<'a>, Vec<Term<'a>>) = term.decode().ok()?;
        let resource: ResourceArc<SchemaResource> = schema.decode().ok()?;
        let mut chunks: Vec<Arc<Chunk<'a>>> = Vec::with_capacity(binaries.len());
        for binary in binaries.iter().rev() {
            let bytes = Binary::from_term(*binary).ok()?.as_slice();
            let imports = Header::read(bytes)
                .ok()?
                .imports
                .iter()
                .map(|id| chunks.iter().find(|chunk| chunk.id() == *id).cloned())
                .collect::<Option<Vec<_>>>()?;
            let chunk = Chunk::load(Cow::Borrowed(bytes), &resource.0, imports).ok()?;
            chunks.push(Arc::new(chunk));
        }
        NodeRef::root(chunks.last()?)?;
        Some(Doc {
            term,
            schema,
            binaries,
            chunks,
        })
    }

    /// The document a ref names that `doc` may have been changed from, which a JSON was written
    /// for. `None` for a term that isn't a ref, or one in another schema or not read where `doc`
    /// was, which shares no node with `doc`: every document changed from another keeps its oldest
    /// chunk. `None` too when `doc` holds none of its newer chunks, having taken them all in, and
    /// its reading flagged no node: it then shares with `doc` only nodes read, whose maps the map
    /// read holds, and that map is shared without loading this one.
    pub fn load_beside(term: Term<'a>, doc: &Doc<'a>) -> Option<Doc<'a>> {
        let (schema, binaries): (Term<'a>, Vec<Term<'a>>) = term.decode().ok()?;
        let (oldest, newer) = binaries.split_last()?;
        let oldest = Binary::from_term(*oldest).ok()?.as_slice();
        if schema != doc.schema || !std::ptr::eq(oldest, doc.chunks[0].bytes()) {
            return None;
        }
        let held = |binary: &Term<'a>| {
            Binary::from_term(*binary).is_ok_and(|binary| {
                let bytes = binary.as_slice();
                doc.chunks[1..]
                    .iter()
                    .any(|chunk| std::ptr::eq(bytes, chunk.bytes()))
            })
        };
        if !newer.iter().any(held) && !doc.chunks[0].any_flagged() {
            return None;
        }
        Doc::load(term)
    }

    pub fn root(&self) -> Node<'a> {
        let newest = self.chunks.last().expect("a chunk");
        Node::root(newest.clone()).expect("a node")
    }

    pub fn view(&self) -> NodeRef<'_> {
        NodeRef::root(self.chunks.last().expect("a chunk")).expect("a node")
    }

    /// The document read from JSON.
    pub fn read(&self) -> NodeRef<'_> {
        NodeRef::root(&self.chunks[0]).expect("a node")
    }

    /// The ref of `doc`, which this document was changed into.
    pub fn changed(&self, env: Env<'a>, doc: &Node<'a>) -> Term<'a> {
        if doc.ptr_eq(&self.root()) {
            return self.term;
        }
        let (flat, kept) = self.flatten(doc);
        let mut binaries = Vec::with_capacity(kept + 1);
        binaries.push(binary(env, flat.chunk().bytes()));
        binaries.extend_from_slice(&self.binaries[self.binaries.len() - kept..]);
        (self.schema, binaries).encode(env)
    }

    /// `doc` in a new chunk, and how many of the oldest chunks it refers into. The chunks the
    /// change wrote, which hold no less than the new chunk will, tell how big it is before it's
    /// written, so that it's written once.
    fn flatten(&self, doc: &Node<'a>) -> (Node<'a>, usize) {
        // The chunks the change wrote: those `doc` imports, and theirs, up to the chunks held,
        // which import only one another. A chunk is known by its address, and the many a change
        // deep down writes are found again in a set.
        let held = |chunk: &Chunk| self.chunks.iter().any(|held| std::ptr::eq(&**held, chunk));
        let (mut size, mut written, mut many) = (0, Vec::new(), HashSet::new());
        let mut next = vec![&**doc.chunk()];
        while let Some(chunk) = next.pop() {
            let address = std::ptr::from_ref(chunk).addr();
            let seen = match written.len() < WRITTEN_SEARCHED {
                true => written.contains(&address),
                false => !many.insert(address),
            };
            if seen || held(chunk) {
                continue;
            }
            if written.len() < WRITTEN_SEARCHED {
                written.push(address);
                if written.len() == WRITTEN_SEARCHED {
                    many.extend(written.iter().copied());
                }
            }
            size += chunk.bytes().len();
            next.extend(chunk.imports().iter().map(|import| &**import));
        }
        let mut kept = self.chunks.len();
        while kept > 1 {
            let newest = self.chunks[kept - 1].bytes().len();
            if kept < MOST_CHUNKS && newest > 2 * size {
                break;
            }
            kept -= 1;
            size += newest;
        }
        (doc.flatten(&self.chunks[..kept]), kept)
    }
}
