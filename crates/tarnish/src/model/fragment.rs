//! Fragments: a node's children.

use std::fmt;
use std::sync::Arc;

use super::node::Node;
use super::read::Reader;
use super::schema::Schema;
use crate::error::{Error, Result};
use crate::js::Json;
use crate::json::Value;
use crate::stack;
use crate::text::Text;

/// What [`Fragment::nodes_between`] calls for each node: the node, its position, its parent,
/// and its index in the parent. Returning `false` skips the node's children.
pub type NodeVisitor<'a> = dyn FnMut(&Node, usize, Option<&Node>, usize) -> Result<bool> + 'a;

/// What [`Fragment::text_between`] calls for a leaf node that isn't text.
pub type LeafTextHook<'a> = dyn FnMut(&Node) -> Result<Text> + 'a;

/// A node's children. Like nodes, fragments are persistent: changing one makes a new one.
#[derive(Clone)]
pub struct Fragment {
    /// `None` for no children, as a leaf has, so that making or dropping a leaf touches no count
    /// shared between threads.
    children: Option<Arc<[Node]>>,
    size: usize,
}

impl Fragment {
    pub fn empty() -> Fragment {
        Fragment {
            children: None,
            size: 0,
        }
    }

    fn with_size(children: Vec<Node>, size: usize) -> Fragment {
        if children.is_empty() {
            return Fragment::empty();
        }
        Fragment {
            children: Some(children.into()),
            size,
        }
    }

    pub(crate) fn new(children: Vec<Node>) -> Fragment {
        let size = children.iter().map(Node::node_size).sum();
        Fragment::with_size(children, size)
    }

    /// [`new`](Self::new), moving the children straight into the fragment's one allocation.
    pub(crate) fn from_drain(children: std::vec::Drain<'_, Node>) -> Fragment {
        if children.len() == 0 {
            return Fragment::empty();
        }
        let children: Arc<[Node]> = children.collect();
        Fragment {
            size: children.iter().map(Node::node_size).sum(),
            children: Some(children),
        }
    }

    /// A fragment of these nodes, joining adjacent text nodes with the same marks.
    pub fn from_array(mut nodes: Vec<Node>) -> Fragment {
        let size = nodes.iter().map(Node::node_size).sum();
        nodes.dedup_by(|next, last| match last.join_text(next) {
            Some(joined) => {
                *last = joined;
                true
            }
            None => false,
        });
        Fragment::with_size(nodes, size)
    }

    pub fn from_node(node: Node) -> Fragment {
        let size = node.node_size();
        Fragment::with_size(vec![node], size)
    }

    /// An identity for the fragment, the same for clones of it.
    pub fn id(&self) -> usize {
        self.children().as_ptr() as usize
    }

    /// Whether this is the very same fragment as `other`, not just an equal one.
    pub fn ptr_eq(&self, other: &Fragment) -> bool {
        match (&self.children, &other.children) {
            (Some(children), Some(others)) => Arc::ptr_eq(children, others),
            (None, None) => true,
            _ => false,
        }
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn children(&self) -> &[Node] {
        self.children.as_deref().unwrap_or(&[])
    }

    pub fn child_count(&self) -> usize {
        self.children().len()
    }

    /// The child at `index`, raising an error when there is none.
    pub fn child(&self, index: usize) -> Result<&Node> {
        match self.children().get(index) {
            Some(child) => Ok(child),
            None => Err(Error::Range(format!(
                "Index {index} out of range for {}",
                self.to_debug_string()?
            ))),
        }
    }

    pub fn maybe_child(&self, index: usize) -> Option<&Node> {
        self.children().get(index)
    }

    pub fn first_child(&self) -> Option<&Node> {
        self.children().first()
    }

    pub fn last_child(&self) -> Option<&Node> {
        self.children().last()
    }

    /// The children, each with its offset in the fragment.
    pub fn children_with_offsets(&self) -> impl Iterator<Item = (usize, &Node)> {
        self.children().iter().scan(0, |offset, child| {
            let at = *offset;
            *offset += child.node_size();
            Some((at, child))
        })
    }

    /// Call `f` for each node, at any depth, between `from` and `to`, counting positions from
    /// `node_start`.
    pub fn nodes_between(
        &self,
        from: usize,
        to: usize,
        f: &mut NodeVisitor,
        node_start: usize,
        parent: Option<&Node>,
    ) -> Result<()> {
        let mut pos = 0;
        for (index, child) in self.children().iter().enumerate() {
            if pos >= to {
                break;
            }
            let end = pos + child.node_size();
            if end > from
                && f(child, node_start + pos, parent, index)?
                && child.content().size() > 0
            {
                let start = pos + 1;
                stack::grow(|| {
                    child.nodes_between(
                        from.saturating_sub(start),
                        child.content().size().min(to - start),
                        f,
                        node_start + start,
                    )
                })?;
            }
            pos = end;
        }
        Ok(())
    }

    pub fn descendants(&self, f: &mut NodeVisitor) -> Result<()> {
        self.nodes_between(0, self.size, f, 0, None)
    }

    /// The text between `from` and `to`. See [`Node::text_between`].
    pub fn text_between(
        &self,
        from: usize,
        to: usize,
        block_separator: Option<&Text>,
        mut leaf_text: Option<&mut LeafTextHook>,
    ) -> Result<Text> {
        let separator = block_separator.filter(|separator| !separator.is_empty());
        let mut parts: Vec<Text> = Vec::new();
        let mut first = true;
        self.nodes_between(
            from,
            to,
            &mut |node, pos, _, _| {
                let node_text = if let Some(own) = node.text() {
                    own.slice(from.max(pos) - pos, to - pos)
                } else if !node.is_leaf() {
                    Text::default()
                } else if let Some(leaf_text) = leaf_text.as_mut() {
                    leaf_text(node)?
                } else if let Some(hook) = &node.node_type().spec().leaf_text {
                    hook(node)?
                } else {
                    Text::default()
                };
                if let Some(separator) = separator
                    && node.is_block()
                    && ((node.is_leaf() && !node_text.is_empty()) || node.is_textblock())
                {
                    if first {
                        first = false;
                    } else {
                        parts.push(separator.clone());
                    }
                }
                parts.push(node_text);
                Ok(true)
            },
            0,
            None,
        )?;
        Ok(parts.iter().collect())
    }

    /// This fragment followed by `other`, joining the text at the seam when it has the same
    /// marks on both sides.
    pub fn append(&self, other: &Fragment) -> Fragment {
        if other.size == 0 {
            return self.clone();
        }
        if self.size == 0 {
            return other.clone();
        }
        let mut content = Vec::with_capacity(self.children().len() + other.children().len());
        content.extend_from_slice(self.children());
        let mut rest = other.children();
        if let Some(last) = content.last_mut()
            && let Some(joined) = last.join_text(&rest[0])
        {
            *last = joined;
            rest = &rest[1..];
        }
        content.extend_from_slice(rest);
        Fragment::with_size(content, self.size + other.size)
    }

    /// The part of the fragment between `from` and `to`.
    pub fn cut(&self, from: usize, to: usize) -> Fragment {
        if from == 0 && to == self.size {
            return self.clone();
        }
        let mut result = Vec::new();
        let mut size = 0;
        if to > from {
            let mut pos = 0;
            for child in self.children().iter() {
                if pos >= to {
                    break;
                }
                let end = pos + child.node_size();
                if end > from {
                    let child = if pos < from || end > to {
                        child
                            .cut_text(from.saturating_sub(pos), to - pos)
                            .unwrap_or_else(|| {
                                stack::grow(|| {
                                    child.cut_content(
                                        from.saturating_sub(pos + 1),
                                        child.content().size().min(to - pos - 1),
                                    )
                                })
                            })
                    } else {
                        child.clone()
                    };
                    size += child.node_size();
                    result.push(child);
                }
                pos = end;
            }
        }
        Fragment::with_size(result, size)
    }

    /// The children from index `from` to `to`.
    pub fn cut_by_index(&self, from: usize, to: usize) -> Fragment {
        let to = to.min(self.children().len());
        let from = from.min(to);
        if from == to {
            return Fragment::empty();
        }
        if from == 0 && to == self.children().len() {
            return self.clone();
        }
        Fragment::new(self.children()[from..to].to_vec())
    }

    /// The fragment with the child at `index` replaced by `node`.
    pub fn replace_child(&self, index: usize, node: Node) -> Fragment {
        let current = &self.children()[index];
        if current.ptr_eq(&node) {
            return self.clone();
        }
        let size = self.size + node.node_size() - current.node_size();
        let mut copy = self.children().to_vec();
        copy[index] = node;
        Fragment::with_size(copy, size)
    }

    /// [`replace_child`](Self::replace_child), in place when no other fragment holds these
    /// children.
    pub(crate) fn set_child(&mut self, index: usize, node: Node) {
        match self.children.as_mut().and_then(Arc::get_mut) {
            Some(children) => {
                self.size = self.size + node.node_size() - children[index].node_size();
                children[index] = node;
            }
            None => *self = self.replace_child(index, node),
        }
    }

    pub fn add_to_start(&self, node: Node) -> Fragment {
        let size = self.size + node.node_size();
        let mut children = Vec::with_capacity(self.children().len() + 1);
        children.push(node);
        children.extend_from_slice(self.children());
        Fragment::with_size(children, size)
    }

    pub fn add_to_end(&self, node: Node) -> Fragment {
        let size = self.size + node.node_size();
        let mut children = self.children().to_vec();
        children.push(node);
        Fragment::with_size(children, size)
    }

    /// The index of the child at `pos`, and that child's offset. At the end of a child, the
    /// index after it.
    pub fn find_index(&self, pos: usize) -> Result<(usize, usize)> {
        if pos == 0 {
            return Ok((0, pos));
        }
        if pos == self.size {
            return Ok((self.children().len(), pos));
        }
        if pos > self.size {
            return Err(Error::Range(format!(
                "Position {pos} outside of fragment ({})",
                self.to_debug_string()?
            )));
        }
        let mut offset = 0;
        for (index, child) in self.children().iter().enumerate() {
            let end = offset + child.node_size();
            if end >= pos {
                if end == pos {
                    return Ok((index + 1, end));
                }
                return Ok((index, offset));
            }
            offset = end;
        }
        unreachable!("a position inside the fragment is inside a child")
    }

    /// `toString`: the children, described, in angle brackets.
    pub fn to_debug_string(&self) -> Result<String> {
        let mut out = String::from("<");
        self.write_debug(&mut out)?;
        out.push('>');
        Ok(out)
    }

    /// `toStringInner`: the children described, without the angle brackets.
    pub fn to_string_inner(&self) -> Result<String> {
        let mut out = String::new();
        self.write_debug(&mut out)?;
        Ok(out)
    }

    pub(crate) fn write_debug(&self, out: &mut String) -> Result<()> {
        for (index, child) in self.children().iter().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            stack::grow(|| child.write_debug(out))?;
        }
        Ok(())
    }

    /// The children as JSON, or `null` when there are none.
    pub fn to_json(&self) -> Value {
        if self.children().is_empty() {
            return Value::Null;
        }
        Value::Array(
            self.children()
                .iter()
                .map(|child| stack::grow(|| child.to_json()))
                .collect(),
        )
    }

    pub fn from_json<'a>(schema: &Schema, json: impl Json<'a>) -> Result<Fragment> {
        Reader::new(schema).fragment(json)
    }
}

impl PartialEq for Fragment {
    fn eq(&self, other: &Fragment) -> bool {
        self.child_count() == other.child_count()
            && self
                .children()
                .iter()
                .zip(other.children())
                .all(|(a, b)| stack::grow(|| a == b))
    }
}

impl fmt::Debug for Fragment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.to_debug_string() {
            Ok(described) => f.write_str(&described),
            Err(error) => write!(f, "<{error}>"),
        }
    }
}
