//! A document's JSON as a term that shares the parts of a map it was read from, or written for: a
//! node that is the very node read from a map, which holds what `toJSON` writes for it, or the
//! very node a map was written for, is that map again.

use rustler::{Env, Term};
use tarnish::js::stack;
use tarnish::{Field, Fields, NodeId, NodeRef};

use crate::term::Writer;

/// A rebuilt map or list item costs about as much as this many bytes of a term.
const BYTES_PER_ITEM: usize = 64;

/// A node's JSON made anew costs about as much as this many bytes a position.
const BYTES_PER_POSITION: usize = 4;

/// Reading past this many children for one that was read finds it too rarely to pay.
const LOOK_AHEAD: usize = 32;

/// Past the work a limit allows.
pub struct TooBig;

/// What the map a document's JSON shares is to the document `read` paired with it.
#[derive(Clone, Copy, PartialEq)]
pub enum Base {
    /// The map `read` was read from, whose parts are what `toJSON` writes for the nodes read from
    /// them, unless their reading flagged them.
    Read,
    /// The map written for `read`, whose every part is what `toJSON` writes for its node.
    Written,
}

/// The JSON of `doc`, `read` being the document read from the map `json`, or that it was written
/// for, doing no more than `limit` bytes' worth of work. A node of `read` is its map again,
/// unless, for `Base::Read`, its reading flagged it.
pub fn json<'a, 'v>(
    env: Env<'a>,
    doc: NodeRef<'v>,
    read: NodeRef<'v>,
    json: Term<'a>,
    base: Base,
    limit: usize,
) -> Result<Term<'a>, TooBig> {
    let mut sharer = Sharer {
        writer: Writer::new(env),
        left: limit,
        flags: base == Base::Read,
    };
    sharer.node(doc.id(), Some((read.id(), json)))
}

struct Sharer<'a, 'v> {
    writer: Writer<'a, 'v>,
    /// The work left, in bytes.
    left: usize,
    /// Whether a node read that its reading flagged is written anew, its map not being what
    /// `toJSON` writes for it.
    flags: bool,
}

impl<'a, 'v> Sharer<'a, 'v> {
    fn spend(&mut self, bytes: usize) -> Result<(), TooBig> {
        self.left = self.left.checked_sub(bytes).ok_or(TooBig)?;
        Ok(())
    }

    /// The node's term, `read` being the node read that it may be, or may have replaced, and
    /// the map it was read from.
    fn node(
        &mut self,
        node: NodeId<'v>,
        read: Option<(NodeId<'v>, Term<'a>)>,
    ) -> Result<Term<'a>, TooBig> {
        if let Some((read, map)) = read
            && read.same(node)
            && !(self.flags && read.flagged())
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
            _ => {
                self.spend(node.node_size() * BYTES_PER_POSITION)?;
                Ok(self.writer.fields(node))
            }
        }
    }

    /// A new map for the node, whose children may be those of the node read.
    fn rebuild(
        &mut self,
        node: NodeRef<'v>,
        read: NodeRef<'v>,
        map: Term<'a>,
    ) -> Result<Term<'a>, TooBig> {
        let content = self.writer.name("content");
        let count = read.child_count() as usize;
        let read_items = map
            .map_get(content)
            .ok()
            .map(|list| items(list, count))
            .filter(|items| items.len() == count);
        self.spend(BYTES_PER_ITEM)?;
        let base = self.writer.begin_map();
        for field in node.fields() {
            let value = match field {
                Field::Type(name) => self.writer.name(name),
                Field::Content(node) => self.children(node, read, read_items.as_deref())?,
                Field::Attrs(attrs) => {
                    self.spend(BYTES_PER_ITEM)?;
                    self.writer.value_ref(attrs)
                }
                Field::Marks(marks) => {
                    self.spend(BYTES_PER_ITEM)?;
                    let items = self.writer.begin_list();
                    for mark in marks.iter() {
                        let term = self.writer.fields(mark);
                        self.writer.item(term);
                    }
                    self.writer.list(items)
                }
                Field::Text(text) => self.writer.text(&text.to_string_lossy()),
            };
            self.writer.entry(field.key(), value);
        }
        Ok(self.writer.map(base))
    }

    /// The list of the children's terms, each paired with the child of `read` it is, or the one
    /// in its place.
    fn children(
        &mut self,
        node: NodeRef<'v>,
        read: NodeRef<'v>,
        read_items: Option<&[Term<'a>]>,
    ) -> Result<Term<'a>, TooBig> {
        let read_children: Vec<NodeId> = match read_items {
            Some(_) => read.child_ids().collect(),
            None => Vec::new(),
        };
        let children: Vec<NodeId> = node.child_ids().collect();
        let base = self.writer.begin_list();
        let mut next = 0;
        // The child read that the last child made anew took the place of, which those made anew
        // right after it, such as the halves of a split, may hold children of too.
        let mut replaced = None;
        for (at, &child) in children.iter().enumerate() {
            self.spend(BYTES_PER_ITEM)?;
            // Most children are the very child read in the same place.
            if let (Some(read), Some(items)) = (read_children.get(next), read_items)
                && read.same(child)
                && !(self.flags && read.flagged())
            {
                self.writer.item(items[next]);
                next += 1;
                replaced = None;
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
                    replaced = None;
                    Some(index)
                }
                // A child made anew takes the place of the next child read, unless that one is
                // still to come. Pairing the children after it with that one again would pair
                // each with a child it isn't, and write the whole of each anew.
                None => {
                    let to_come = |read: &NodeId| {
                        children[at + 1..]
                            .iter()
                            .take(LOOK_AHEAD)
                            .any(|child| child.same(*read))
                    };
                    if read_children.get(next).is_some_and(|read| !to_come(read)) {
                        replaced = Some(next);
                        next += 1;
                    }
                    replaced
                }
            };
            let pair = index
                .zip(read_items)
                .map(|(index, items)| (read_children[index], items[index]));
            let term = self.node(child, pair)?;
            self.writer.item(term);
        }
        Ok(self.writer.list(base))
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
