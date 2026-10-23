//! A document's JSON as a term that shares the parts of a map it was read from, or written for: a
//! node that is the very node read from a map, which holds what `toJSON` writes for it, or the
//! very node a map was written for, is that map again.

use rustler::{Env, Term};
use tarnish::js::stack;
use tarnish::{Change, Field, Fields, NodeId, NodeRef};

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

/// The JSON of the document `change` makes, `read` being the document read from the map `json`,
/// or that it was written for, or none, doing no more than `limit` bytes' worth of work. A node
/// of `read` is its map again, unless, for `Base::Read`, its reading flagged it. The nodes along
/// the change's path are written from those of the document it changes, without their being
/// written into a chunk first.
pub fn json<'a, 'v>(
    env: Env<'a>,
    change: &'v Change<'_>,
    read: Option<NodeRef<'v>>,
    json: Term<'a>,
    base: Base,
    limit: usize,
) -> Result<Term<'a>, TooBig> {
    let mut sharer = Sharer {
        writer: Writer::new(env),
        left: limit,
        base,
    };
    let read = read.map(|read| (read.id(), json));
    sharer.path(
        change.top().view(),
        change.path(),
        change.bottom().view(),
        read,
    )
}

struct Sharer<'a, 'v> {
    writer: Writer<'a, 'v>,
    /// The work left, in bytes.
    left: usize,
    /// What the map shared is to the document read: for `Base::Read`, a node read that its
    /// reading flagged is written anew, its map not being what `toJSON` writes for it.
    base: Base,
}

impl<'a, 'v> Sharer<'a, 'v> {
    /// Whether the map of node `read` is `node`'s JSON.
    #[inline]
    fn is_its_map(&self, read: NodeId<'v>, node: NodeId<'v>) -> bool {
        read.same(node) && (self.base == Base::Written || !read.flagged())
    }

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
            && self.is_its_map(read, node)
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
                stack::grow(|| self.rebuild(node, Some((read, map)), None))
            }
            _ => {
                self.spend(node.node_size() * BYTES_PER_POSITION)?;
                Ok(self.writer.fields(node))
            }
        }
    }

    /// The term of `node` with the descendant `path` leads to replaced by `bottom`, `read`
    /// being the node read that `node` may be, and its map. With no path, it is `bottom`'s.
    fn path(
        &mut self,
        node: NodeRef<'v>,
        path: &[u32],
        bottom: NodeRef<'v>,
        read: Option<(NodeId<'v>, Term<'a>)>,
    ) -> Result<Term<'a>, TooBig> {
        if path.is_empty() {
            return self.node(bottom.id(), read);
        }
        let read = read.map(|(read, map)| (read.view(), map));
        let read = read.filter(|(read, _)| read.node_type() == node.node_type());
        stack::grow(|| self.rebuild(node, read, Some((path, bottom))))
    }

    /// A new map for the node, whose children may be those of the node read, and the child at
    /// `path[0]` replaced as [`path`](Self::path) replaces it.
    fn rebuild(
        &mut self,
        node: NodeRef<'v>,
        read: Option<(NodeRef<'v>, Term<'a>)>,
        path: Option<(&[u32], NodeRef<'v>)>,
    ) -> Result<Term<'a>, TooBig> {
        let content = self.writer.name("content");
        let read_items = read.and_then(|(read, map)| {
            let count = read.child_count() as usize;
            map.map_get(content)
                .ok()
                .map(|list| items(list, count))
                .filter(|items| items.len() == count)
        });
        self.spend(BYTES_PER_ITEM)?;
        let base = self.writer.begin_map();
        for field in node.fields() {
            let value = match field {
                Field::Type(name) => self.writer.name(name),
                Field::Content(node) => {
                    let read = read.map(|(read, _)| read);
                    self.children(node, read, read_items.as_deref(), path)?
                }
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
        read: Option<NodeRef<'v>>,
        read_items: Option<&[Term<'a>]>,
        path: Option<(&[u32], NodeRef<'v>)>,
    ) -> Result<Term<'a>, TooBig> {
        let read_children: Vec<NodeId> = match (read_items, read) {
            (Some(_), Some(read)) => read.child_ids().collect(),
            _ => Vec::new(),
        };
        let children: Vec<NodeId> = node.child_ids().collect();
        let base = self.writer.begin_list();
        let paired = pairing(&children, &read_children, |a, b| a.same(b));
        for (at, (&child, index)) in children.iter().zip(paired).enumerate() {
            self.spend(BYTES_PER_ITEM)?;
            let replaced = path.filter(|(path, _)| path[0] as usize == at);
            // Most children are the very child read in the same place.
            if let (Some(index), Some(items), None) = (index, read_items, replaced)
                && self.is_its_map(read_children[index], child)
            {
                self.writer.item(items[index]);
                continue;
            }
            let pair = index
                .zip(read_items)
                .map(|(index, items)| (read_children[index], items[index]));
            let term = match replaced {
                Some((path, bottom)) => self.path(child.view(), &path[1..], bottom, pair)?,
                None => self.node(child, pair)?,
            };
            self.writer.item(term);
        }
        Ok(self.writer.list(base))
    }
}

/// For each of `children`, the index of the child of `read` it is, or the one in its place,
/// `same` telling whether two are the same node.
fn pairing<'s, T: Copy, F: Fn(T, T) -> bool>(
    children: &'s [T],
    read: &'s [T],
    same: F,
) -> Pairing<'s, T, F> {
    Pairing {
        children,
        read,
        same,
        at: 0,
        next: 0,
        replaced: None,
    }
}

struct Pairing<'s, T, F> {
    children: &'s [T],
    read: &'s [T],
    same: F,
    /// The child paired next.
    at: usize,
    /// The child read after the last one paired.
    next: usize,
    /// The child read that the last child made anew took the place of, which those made anew
    /// right after it, such as the halves of a split, may hold children of too.
    replaced: Option<usize>,
}

impl<T: Copy, F: Fn(T, T) -> bool> Iterator for Pairing<'_, T, F> {
    type Item = Option<usize>;

    #[inline]
    fn next(&mut self) -> Option<Option<usize>> {
        let at = self.at;
        let child = *self.children.get(at)?;
        self.at += 1;
        let same = &self.same;
        // Most children are the very child read in the same place.
        if let Some(&read) = self.read.get(self.next)
            && same(read, child)
        {
            let index = self.next;
            self.next += 1;
            self.replaced = None;
            return Some(Some(index));
        }
        let next = self.next;
        let found = self.read[next..]
            .iter()
            .take(LOOK_AHEAD)
            .position(|&read| same(read, child))
            .map(|offset| next + offset);
        if let Some(index) = found {
            self.next = index + 1;
            self.replaced = None;
            return Some(Some(index));
        }
        // A child made anew takes the place of the next child read, unless that one is still
        // to come. Pairing the children after it with that one again would pair each with a
        // child it isn't, and write the whole of each anew.
        let to_come = |read: T| {
            self.children[at + 1..]
                .iter()
                .take(LOOK_AHEAD)
                .any(|&child| same(child, read))
        };
        if self.read.get(next).is_some_and(|&read| !to_come(read)) {
            self.replaced = Some(next);
            self.next += 1;
        }
        Some(self.replaced)
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

#[cfg(test)]
mod tests {
    use super::pairing;

    fn paired(children: &[u32], read: &[u32]) -> Vec<Option<usize>> {
        pairing(children, read, |a, b| a == b).collect()
    }

    #[test]
    fn unchanged_children_pair_in_place() {
        assert_eq!(paired(&[1, 2, 3], &[1, 2, 3]), [Some(0), Some(1), Some(2)]);
    }

    #[test]
    fn an_inserted_child_takes_the_place_of_none_still_to_come() {
        assert_eq!(
            paired(&[1, 9, 2, 3], &[1, 2, 3]),
            [Some(0), None, Some(1), Some(2)]
        );
    }

    #[test]
    fn a_deleted_child_is_passed() {
        assert_eq!(paired(&[1, 3], &[1, 2, 3]), [Some(0), Some(2)]);
    }

    #[test]
    fn the_halves_of_a_split_take_the_place_of_the_child_split() {
        assert_eq!(
            paired(&[1, 8, 9, 3], &[1, 2, 3]),
            [Some(0), Some(1), Some(1), Some(2)]
        );
    }

    #[test]
    fn a_child_replaced_deep_down_is_followed_by_those_unchanged() {
        assert_eq!(
            paired(&[1, 8, 3, 4], &[1, 2, 3, 4]),
            [Some(0), Some(1), Some(2), Some(3)]
        );
    }

    #[test]
    fn a_child_made_anew_takes_no_place_of_one_still_to_come() {
        assert_eq!(paired(&[8, 1, 2], &[1, 2]), [None, Some(0), Some(1)]);
    }

    #[test]
    fn nothing_read_pairs_with_nothing() {
        assert_eq!(paired(&[1, 2], &[]), [None, None]);
    }
}
