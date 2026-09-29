//! A document as Elixir holds it: `{schema, chunks}`, the chunks being binaries, newest first.
//! The newest chunk's last node is the document. The oldest is the chunk read from the JSON the
//! document keeps, whose maps its own JSON shares.
//!
//! A change writes one chunk more, which refers into the ones before it for what it didn't
//! change. That chunk takes in the newer chunks it outgrows, so that each chunk is more than
//! twice the size of the one after it, and there are few of them.

use std::borrow::Cow;
use std::sync::Arc;

use rustler::{Binary, Encoder, Env, NewBinary, ResourceArc, Term};
use tarnish::chunk::{Chunk, Header};
use tarnish::{Node, NodeRef};

use crate::prosemirror::SchemaResource;

/// The most chunks a document is held in.
const MOST_CHUNKS: usize = 8;

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
        let held = |chunk: &Arc<Chunk>| self.chunks.iter().any(|held| held.ptr_eq(chunk));
        let mut size: usize = Chunk::closure(doc.chunk())
            .iter()
            .filter(|chunk| !held(chunk))
            .map(|chunk| chunk.bytes().len())
            .sum();
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
