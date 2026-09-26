//! A document's JSON as a term that shares the parts of the map it was read from: a node that is
//! the very node read from a map, which holds what `toJSON` writes for it, is that map again.

use std::collections::HashSet;

use rustler::{Encoder, Env, Term};
use tarnish::{Field, Fields, Mark, Node, Value, stack};

use crate::etf;

/// A document as read from a map, and the nodes whose maps aren't what `toJSON` writes.
pub struct Source {
    pub doc: Node,
    pub irregular: HashSet<usize>,
}

/// A rebuilt map or list item costs about as much as this many bytes of a term.
const BYTES_PER_ITEM: usize = 64;

/// Reading past this many children for one that was read finds it too rarely to pay.
const LOOK_AHEAD: usize = 32;

/// Past the work a limit allows.
pub struct TooBig;

/// The JSON of `doc`, the map `json` being the one `source` was read from, doing no more than
/// `limit` bytes' worth of work.
pub fn json<'a>(
    env: Env<'a>,
    doc: &Node,
    source: &Source,
    json: Term<'a>,
    limit: usize,
) -> Result<Term<'a>, TooBig> {
    let mut sharer = Sharer {
        env,
        source,
        keys: Vec::new(),
        left: limit,
    };
    sharer.node(doc, Some((&source.doc, json)))
}

struct Sharer<'a, 's> {
    env: Env<'a>,
    source: &'s Source,
    /// Keys and type names made so far, by the address of their text.
    keys: Vec<(usize, Term<'a>)>,
    /// The work left, in bytes.
    left: usize,
}

impl<'a> Sharer<'a, '_> {
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
    fn node(&mut self, node: &Node, read: Option<(&Node, Term<'a>)>) -> Result<Term<'a>, TooBig> {
        match read {
            Some((read, map))
                if read.ptr_eq(node) && !self.source.irregular.contains(&read.id()) =>
            {
                Ok(map)
            }
            Some((read, map))
                if read.node_type() == node.node_type()
                    && read.child_count() > 0
                    && node.child_count() > 0 =>
            {
                self.rebuild(node, read, map)
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
    fn rebuild(&mut self, node: &Node, read: &Node, map: Term<'a>) -> Result<Term<'a>, TooBig> {
        let content = self.key("content");
        let read_items = map
            .map_get(content)
            .ok()
            .map(items)
            .filter(|items| items.len() == read.child_count());
        let (mut keys, mut values) = (Vec::with_capacity(5), Vec::with_capacity(5));
        let mut failed = Ok(());
        node.fields(|field| {
            if failed.is_err() {
                return;
            }
            let value = match field {
                Field::Type(name) => Ok(self.key(name)),
                Field::Content(children) => self.children(children, read, read_items.as_deref()),
                Field::Attrs(attrs) => {
                    self.written(etf::write(&Value::Object(attrs.clone()), self.left))
                }
                Field::Marks(marks) => {
                    let marks = Value::Array(marks.iter().map(Mark::to_json).collect());
                    self.written(etf::write(&marks, self.left))
                }
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
        children: &[Node],
        read: &Node,
        read_items: Option<&[Term<'a>]>,
    ) -> Result<Term<'a>, TooBig> {
        let read_children = read_items.map_or(&[][..], |_| read.children());
        let mut terms = Vec::with_capacity(children.len());
        let mut next = 0;
        for child in children {
            self.spend(BYTES_PER_ITEM)?;
            let found = read_children[next..]
                .iter()
                .take(LOOK_AHEAD)
                .position(|read| read.ptr_eq(child))
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
                .map(|(index, items)| (&read_children[index], items[index]));
            terms.push(stack::grow(|| self.node(child, pair))?);
        }
        Ok(terms.encode(self.env))
    }
}

/// A proper list's items.
fn items(list: Term) -> Vec<Term> {
    let mut items = Vec::new();
    let mut rest = list;
    while let Ok((head, tail)) = rest.list_get_cell() {
        items.push(head);
        rest = tail;
    }
    items
}
