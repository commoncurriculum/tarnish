//! A document's JSON as a term that shares the parts of the map it was read from: a node that is
//! the very node read from a map, which holds what `toJSON` writes for it, is that map again.

use rustler::{Encoder, Env, Term};
use tarnish::{Field, Fields, NodeId, NodeRef, stack};

use crate::etf;

/// A rebuilt map or list item costs about as much as this many bytes of a term.
const BYTES_PER_ITEM: usize = 64;

/// Reading past this many children for one that was read finds it too rarely to pay.
const LOOK_AHEAD: usize = 32;

/// Past the work a limit allows.
pub struct TooBig;

/// The JSON of `doc`, `read` being the document read from the map `json`, doing no more than
/// `limit` bytes' worth of work. A node read is its map again unless its reading flagged it.
pub fn json<'a>(
    env: Env<'a>,
    doc: NodeRef,
    read: NodeRef,
    json: Term<'a>,
    limit: usize,
) -> Result<Term<'a>, TooBig> {
    let mut sharer = Sharer {
        env,
        keys: Vec::new(),
        left: limit,
    };
    sharer.node(doc.id(), Some((read.id(), json)))
}

struct Sharer<'a> {
    env: Env<'a>,
    /// Keys and type names made so far, by the address of their text.
    keys: Vec<(usize, Term<'a>)>,
    /// The work left, in bytes.
    left: usize,
}

impl<'a> Sharer<'a> {
    fn spend(&mut self, bytes: usize) -> Result<(), TooBig> {
        self.left = self.left.checked_sub(bytes).ok_or(TooBig)?;
        Ok(())
    }

    fn key(&mut self, text: &str) -> Term<'a> {
        let address = text.as_ptr() as usize;
        if let Some(&(_, key)) = self.keys.iter().find(|(known, _)| *known == address) {
            return key;
        }
        let key = text.encode(self.env);
        self.keys.push((address, key));
        key
    }

    /// The node's term, `read` being the node read that it may be, or may have replaced, and
    /// the map it was read from.
    fn node(&mut self, node: NodeId, read: Option<(NodeId, Term<'a>)>) -> Result<Term<'a>, TooBig> {
        if let Some((read, map)) = read
            && read.same(node)
            && !read.flagged()
        {
            return Ok(map);
        }
        let node = node.view();
        match read.map(|(read, map)| (read.view(), map)) {
            Some((read, map))
                if read.node_type() == node.node_type()
                    && read.child_count() > 0
                    && node.child_count() > 0 =>
            {
                stack::grow(|| self.rebuild(node, read, map))
            }
            _ => self.written(etf::write_node(node, self.left)),
        }
    }

    fn written(&mut self, bytes: Option<Vec<u8>>) -> Result<Term<'a>, TooBig> {
        let bytes = bytes.ok_or(TooBig)?;
        self.spend(bytes.len())?;
        Ok(crate::term::make(self.env, &bytes))
    }

    /// A new map for the node, whose children may be those of the node read.
    fn rebuild(&mut self, node: NodeRef, read: NodeRef, map: Term<'a>) -> Result<Term<'a>, TooBig> {
        let content = self.key("content");
        let count = read.child_count() as usize;
        let read_items = map
            .map_get(content)
            .ok()
            .map(|list| items(list, count))
            .filter(|items| items.len() == count);
        let (mut keys, mut values) = (Vec::with_capacity(5), Vec::with_capacity(5));
        let mut failed = Ok(());
        node.fields(|field| {
            if failed.is_err() {
                return;
            }
            let value = match field {
                Field::Type(name) => Ok(self.key(name)),
                Field::Content(node) => self.children(node, read, read_items.as_deref()),
                Field::Attrs(attrs) => self.written(etf::write_ref(attrs, self.left)),
                Field::Marks(marks) => self.written(etf::write_marks(marks, self.left)),
                Field::Text(text) => Ok(text.to_string_lossy().encode(self.env)),
            };
            match value {
                Ok(value) => {
                    keys.push(self.key(field.key()));
                    values.push(value);
                }
                Err(too_big) => failed = Err(too_big),
            }
        });
        failed?;
        self.spend(BYTES_PER_ITEM)?;
        Ok(Term::map_from_term_arrays(self.env, &keys, &values).expect("a node's keys, each once"))
    }

    /// The list of the children's terms, each paired with the child of `read` it is, or the one
    /// in its place.
    fn children(
        &mut self,
        node: NodeRef,
        read: NodeRef,
        read_items: Option<&[Term<'a>]>,
    ) -> Result<Term<'a>, TooBig> {
        let read_children: Vec<NodeId> = match read_items {
            Some(_) => read.child_ids().collect(),
            None => Vec::new(),
        };
        let mut terms = Vec::with_capacity(node.child_count() as usize);
        let mut next = 0;
        for child in node.child_ids() {
            self.spend(BYTES_PER_ITEM)?;
            // Most children are the very child read in the same place.
            if let (Some(read), Some(items)) = (read_children.get(next), read_items)
                && read.same(child)
                && !read.flagged()
            {
                terms.push(items[next]);
                next += 1;
                continue;
            }
            let found = read_children[next..]
                .iter()
                .take(LOOK_AHEAD)
                .position(|read| read.same(child))
                .map(|offset| next + offset);
            let index = match found {
                Some(index) => {
                    next = index + 1;
                    Some(index)
                }
                None => (next < read_children.len()).then_some(next),
            };
            let pair = index
                .zip(read_items)
                .map(|(index, items)| (read_children[index], items[index]));
            terms.push(self.node(child, pair)?);
        }
        Ok(terms.encode(self.env))
    }
}

/// A proper list's items, of which there are likely `count`.
fn items(list: Term, count: usize) -> Vec<Term> {
    let mut items = Vec::with_capacity(count);
    let mut rest = list;
    while let Ok((head, tail)) = rest.list_get_cell() {
        items.push(head);
        rest = tail;
    }
    items
}
