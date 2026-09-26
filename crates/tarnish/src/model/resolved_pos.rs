//! Resolved positions: a position with the nodes around it.

use std::fmt;

use super::mark::{Mark, Marks};
use super::node::Node;
use crate::error::{Error, Result};

/// What [`ResolvedPos::block_range`] asks whether a range may be in a node.
pub type NodePredicate<'a> = dyn FnMut(&Node) -> Result<bool> + 'a;

/// A position in a document, with its ancestors, its index in each, and where each starts.
#[derive(Clone)]
pub struct ResolvedPos {
    pos: usize,
    /// For each depth: the ancestor, the index into it, and the position where that child starts.
    path: Vec<Step>,
    parent_offset: usize,
}

#[derive(Clone)]
struct Step {
    node: Node,
    index: usize,
    offset: usize,
}

impl ResolvedPos {
    pub(crate) fn resolve(doc: &Node, pos: usize) -> Result<ResolvedPos> {
        if pos > doc.content().size() {
            return Err(Error::Range(format!("Position {pos} out of range")));
        }
        let mut path = Vec::new();
        let mut start = 0;
        let mut parent_offset = pos;
        let mut node = doc;
        loop {
            let (index, offset) = node.content().find_index(parent_offset)?;
            let rem = parent_offset - offset;
            path.push(Step {
                node: node.clone(),
                index,
                offset: start + offset,
            });
            if rem == 0 {
                break;
            }
            node = node.child(index)?;
            if node.is_text() {
                break;
            }
            parent_offset = rem - 1;
            start += offset + 1;
        }
        Ok(ResolvedPos {
            pos,
            path,
            parent_offset,
        })
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    /// How deep the parent is: 0 when the position points into the root node.
    pub fn depth(&self) -> usize {
        self.path.len() - 1
    }

    /// The position's offset in its parent.
    pub fn parent_offset(&self) -> usize {
        self.parent_offset
    }

    /// The node the position points into. A text node is never a parent.
    pub fn parent(&self) -> &Node {
        self.node(self.depth())
    }

    pub fn doc(&self) -> &Node {
        self.node(0)
    }

    /// The ancestor at `depth`.
    pub fn node(&self, depth: usize) -> &Node {
        &self.path[depth].node
    }

    /// The index into the ancestor at `depth`.
    pub fn index(&self, depth: usize) -> usize {
        self.path[depth].index
    }

    /// The index after the position in the ancestor at `depth`.
    pub fn index_after(&self, depth: usize) -> usize {
        self.index(depth) + usize::from(depth != self.depth() || self.text_offset() > 0)
    }

    /// Where the ancestor at `depth`'s content starts.
    pub fn start(&self, depth: usize) -> usize {
        if depth == 0 {
            0
        } else {
            self.path[depth - 1].offset + 1
        }
    }

    /// Where the ancestor at `depth`'s content ends.
    pub fn end(&self, depth: usize) -> usize {
        self.start(depth) + self.node(depth).content().size()
    }

    /// The position before the ancestor at `depth`, or at `depth` one past the parent's, this
    /// position.
    pub fn before(&self, depth: usize) -> Result<usize> {
        if depth == 0 {
            return Err(Error::Range(
                "There is no position before the top-level node".into(),
            ));
        }
        Ok(self.before_nonzero(depth))
    }

    fn before_nonzero(&self, depth: usize) -> usize {
        if depth == self.depth() + 1 {
            self.pos
        } else {
            self.path[depth - 1].offset
        }
    }

    /// The position after the ancestor at `depth`, or at `depth` one past the parent's, this
    /// position.
    pub fn after(&self, depth: usize) -> Result<usize> {
        if depth == 0 {
            return Err(Error::Range(
                "There is no position after the top-level node".into(),
            ));
        }
        Ok(self.after_nonzero(depth))
    }

    fn after_nonzero(&self, depth: usize) -> usize {
        if depth == self.depth() + 1 {
            self.pos
        } else {
            self.path[depth - 1].offset + self.path[depth].node.node_size()
        }
    }

    /// How far into a text node the position is; 0 between nodes.
    pub fn text_offset(&self) -> usize {
        self.pos - self.path.last().expect("a root").offset
    }

    /// The node after the position; when the position is in a text node, the text after it.
    pub fn node_after(&self) -> Option<Node> {
        let parent = self.parent();
        let child = parent.maybe_child(self.index(self.depth()))?;
        match self.text_offset() {
            0 => Some(child.clone()),
            offset => Some(child.cut_within(offset, child.node_size())),
        }
    }

    /// The node before the position; when the position is in a text node, the text before it.
    pub fn node_before(&self) -> Option<Node> {
        let index = self.index(self.depth());
        match self.text_offset() {
            0 if index == 0 => None,
            0 => self.parent().maybe_child(index - 1).cloned(),
            offset => Some(self.parent().maybe_child(index)?.cut_within(0, offset)),
        }
    }

    /// The position at child `index` of the ancestor at `depth`.
    pub fn pos_at_index(&self, index: usize, depth: usize) -> usize {
        let node = self.node(depth);
        node.children()
            .iter()
            .take(index)
            .map(Node::node_size)
            .sum::<usize>()
            + self.start(depth)
    }

    /// The marks at the position: those of the node before it, or at the start of its parent the
    /// node after, but for non-inclusive marks the other node doesn't have.
    pub fn marks(&self) -> Marks {
        let parent = self.parent();
        let index = self.index(self.depth());
        if parent.content().size() == 0 {
            return Mark::none();
        }
        if self.text_offset() > 0 {
            return parent.children()[index].marks().clone();
        }
        let before = index
            .checked_sub(1)
            .and_then(|before| parent.maybe_child(before));
        let after = parent.maybe_child(index);
        let (main, other) = match before {
            Some(before) => (before, after),
            None => (after.expect("a child in a non-empty parent"), None),
        };
        without_exclusive(main.marks(), other)
    }

    /// The marks of the node after the position that go on over a deletion to `end`: all but
    /// the non-inclusive ones the node after `end` doesn't have. `None` when no inline node is
    /// after the position.
    pub fn marks_across(&self, end: &ResolvedPos) -> Option<Marks> {
        let after = self.parent().maybe_child(self.index(self.depth()))?;
        if !after.is_inline() {
            return None;
        }
        let next = end.parent().maybe_child(end.index(end.depth()));
        Some(without_exclusive(after.marks(), next))
    }

    /// The depth up to which this position and `pos` have the same ancestors.
    pub fn shared_depth(&self, pos: usize) -> usize {
        (1..=self.depth())
            .rev()
            .find(|&depth| self.start(depth) <= pos && self.end(depth) >= pos)
            .unwrap_or(0)
    }

    /// The range around the block content where this position and `other` diverge, in the
    /// deepest ancestor `pred` accepts.
    pub fn block_range(
        &self,
        other: &ResolvedPos,
        pred: Option<&mut NodePredicate>,
    ) -> Result<Option<NodeRange>> {
        if other.pos < self.pos {
            return other.block_range(self, None);
        }
        let mut pred = pred;
        let skip = usize::from(self.parent().inline_content() || self.pos == other.pos);
        let Some(top) = self.depth().checked_sub(skip) else {
            return Ok(None);
        };
        for depth in (0..=top).rev() {
            if other.pos <= self.end(depth) {
                let accepted = match pred.as_mut() {
                    Some(pred) => pred(self.node(depth))?,
                    None => true,
                };
                if accepted {
                    return Ok(Some(NodeRange::new(self.clone(), other.clone(), depth)));
                }
            }
        }
        Ok(None)
    }

    pub fn same_parent(&self, other: &ResolvedPos) -> bool {
        self.pos - self.parent_offset == other.pos - other.parent_offset
    }

    pub fn max<'a>(&'a self, other: &'a ResolvedPos) -> &'a ResolvedPos {
        if other.pos > self.pos { other } else { self }
    }

    pub fn min<'a>(&'a self, other: &'a ResolvedPos) -> &'a ResolvedPos {
        if other.pos < self.pos { other } else { self }
    }
}

/// `marks` without the non-inclusive ones `other` doesn't have.
fn without_exclusive(marks: &Marks, other: Option<&Node>) -> Marks {
    let mut marks = marks.clone();
    let mut index = 0;
    while index < marks.len() {
        let mark = &marks[index];
        if mark.mark_type().spec().inclusive == Some(false)
            && !other.is_some_and(|other| mark.is_in_set(other.marks()))
        {
            marks = mark.clone().remove_from_set(&marks);
        } else {
            index += 1;
        }
    }
    marks
}

impl fmt::Display for ResolvedPos {
    /// `toString`: each ancestor's type and index, then the offset into the parent.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for depth in 1..=self.depth() {
            if depth > 1 {
                f.write_str("/")?;
            }
            write!(
                f,
                "{}_{}",
                self.node(depth).node_type().name(),
                self.index(depth - 1)
            )?;
        }
        write!(f, ":{}", self.parent_offset)
    }
}

impl fmt::Debug for ResolvedPos {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ResolvedPos({} {self})", self.pos)
    }
}

/// A flat range of content: adjacent children of one node.
#[derive(Clone, Debug)]
pub struct NodeRange {
    from: ResolvedPos,
    to: ResolvedPos,
    depth: usize,
}

impl NodeRange {
    /// A range of the children of the ancestor at `depth`, which `from` and `to` share.
    pub fn new(from: ResolvedPos, to: ResolvedPos, depth: usize) -> NodeRange {
        NodeRange { from, to, depth }
    }

    /// The position the range was made from at its start, which may be deeper than the range.
    pub fn resolved_from(&self) -> &ResolvedPos {
        &self.from
    }

    /// The position the range was made from at its end, which may be deeper than the range.
    pub fn resolved_to(&self) -> &ResolvedPos {
        &self.to
    }

    /// The depth of the node the range is in.
    pub fn depth(&self) -> usize {
        self.depth
    }

    pub fn start(&self) -> usize {
        self.from.before_nonzero(self.depth + 1)
    }

    pub fn end(&self) -> usize {
        self.to.after_nonzero(self.depth + 1)
    }

    pub fn parent(&self) -> &Node {
        self.from.node(self.depth)
    }

    pub fn start_index(&self) -> usize {
        self.from.index(self.depth)
    }

    pub fn end_index(&self) -> usize {
        self.to.index_after(self.depth)
    }
}
