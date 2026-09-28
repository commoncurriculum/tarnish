//! Checking a document against its schema, as `Node.check` does, reading each node once.

use super::mark::added_to;
use super::node::Node;
use super::view::{MarkRef, NodeRef, SetRef};
use crate::error::{Error, Result};
use crate::stack;

impl Node<'_> {
    /// Raise an error if this node or a descendant doesn't fit the schema.
    pub fn check(&self) -> Result<()> {
        let mut walk = Walk {
            path: Vec::new(),
            kids: Vec::new(),
        };
        match walk.check(self.view()) {
            Ok(()) => Ok(()),
            Err(Failed::Error(error)) => Err(error),
            // The error describes the content, for which the schema's hooks need nodes.
            Err(Failed::Content) => {
                let mut node = self.clone();
                for index in walk.path {
                    node = node.child(index as usize)?;
                }
                node.node_type().check_content(node.content())
            }
        }
    }
}

enum Failed {
    /// The content of the node the path leads to doesn't fit its type.
    Content,
    Error(Error),
}

impl From<Error> for Failed {
    fn from(error: Error) -> Failed {
        Failed::Error(error)
    }
}

/// A walk over a document checking it: the path of child indices to the node it's at, and the
/// children of the nodes on the path, each node's after its parent's.
struct Walk<'c> {
    path: Vec<u32>,
    kids: Vec<NodeRef<'c>>,
}

impl<'c> Walk<'c> {
    fn check(&mut self, node: NodeRef<'c>) -> Result<(), Failed> {
        let node_type = node.node_type();
        let data = node_type.data();
        let base = self.kids.len();
        self.kids.extend(node.children());
        if !data.valid_content(node_type.schema(), self.kids[base..].iter().copied()) {
            return Err(Failed::Content);
        }
        data.attrs.check_ref(node.attrs())?;
        check_marks(node)?;
        for index in base..self.kids.len() {
            let child = self.kids[index];
            // Text has no content and no attributes, so only its marks can be wrong.
            if child.is_text() {
                check_marks(child)?;
                continue;
            }
            self.path.push((index - base) as u32);
            stack::grow(|| self.check(child))?;
            self.path.pop();
        }
        self.kids.truncate(base);
        Ok(())
    }
}

/// Checks a node's marks' attributes, and that they make a set.
#[inline]
fn check_marks(node: NodeRef) -> Result<(), Failed> {
    // Set 0 is the empty set in every chunk.
    if node.record.marks == 0 {
        return Ok(());
    }
    let marks = node.marks();
    let mut count = 0;
    for mark in marks.iter() {
        mark.mark_type().data().attrs.check_ref(mark.attrs())?;
        count += 1;
    }
    // Adding one mark to no marks gives that mark, so only a longer set can be invalid.
    if count > 1 && !valid_set(marks) {
        return Err(Failed::Error(invalid_set(node, marks)));
    }
    Ok(())
}

#[cold]
fn invalid_set(node: NodeRef, marks: SetRef) -> Error {
    let names: Vec<&str> = marks.iter().map(|mark| mark.mark_type().name()).collect();
    Error::Range(format!(
        "Invalid collection of marks for node {}: {}",
        node.node_type().name(),
        names.join(",")
    ))
}

/// Whether adding each mark in turn to no marks gives the set back: sorted by rank, with no
/// mark excluding another, and no mark twice.
fn valid_set(marks: SetRef) -> bool {
    let marks: Vec<MarkRef> = marks.iter().collect();
    let built = marks.iter().fold(Vec::new(), |built, mark| {
        added_to(mark, &built).unwrap_or(built)
    });
    built.len() == marks.len() && built.iter().zip(&marks).all(|(a, b)| a.equals(*b))
}
