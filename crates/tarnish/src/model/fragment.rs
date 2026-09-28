//! Fragments: a node's children.

use std::fmt;
use std::sync::Arc;

use super::node::Node;
use super::read::Reader;
use super::schema::Schema;
use super::view::NodeRef;
use crate::chunk::{Builder, Chunk, Kid};
use crate::error::{Error, Result};
use crate::js::Json;
use crate::json::Value;
use crate::stack;
use crate::text::Text;

/// What [`Fragment::nodes_between`] calls for each node: the node, its position, its parent,
/// and its index in the parent. Returning `false` skips the node's children.
pub type NodeVisitor<'f, 'a> =
    dyn FnMut(&Node<'a>, usize, Option<&Node<'a>>, usize) -> Result<bool> + 'f;

/// What [`Fragment::text_between`] calls for a leaf node that isn't text.
pub type LeafTextHook<'f, 'a> = dyn FnMut(&Node<'a>) -> Result<Text> + 'f;

/// A node's children: a range of a chunk's kids. Like nodes, fragments are persistent: changing
/// one makes a new one.
#[derive(Clone)]
pub struct Fragment<'a> {
    /// `None` for a fragment made empty, which touches no count shared between threads.
    chunk: Option<Arc<Chunk<'a>>>,
    /// The first kid, in the chunk's kids.
    start: u32,
    count: u32,
    size: u32,
    /// Kids in this chunk are nodes before this one: a node's content holds only nodes written
    /// before it.
    bound: u32,
}

/// A fragment's list being written into a chunk of its own: nodes, and runs of other
/// fragments' kids, copied as refs.
struct List<'a> {
    builder: Builder<'a>,
    start: u32,
    count: u32,
}

impl<'a> List<'a> {
    fn new(schema: &Schema) -> List<'a> {
        let builder = Builder::new(schema);
        let start = builder.kids_end();
        List {
            builder,
            start,
            count: 0,
        }
    }

    fn push(&mut self, node: &Node<'a>) {
        let kid = self
            .builder
            .kid(node.chunk(), node.index(), node.node_size());
        self.push_kid(kid);
    }

    fn push_kid(&mut self, kid: Kid) {
        self.builder.push_kid(kid);
        self.count += 1;
    }

    /// Adds `fragment`'s children from index `from` to `to`.
    fn extend(&mut self, fragment: &Fragment<'a>, from: u32, to: u32) {
        if let Some(chunk) = &fragment.chunk
            && from < to
        {
            let (start, bound) = (fragment.start, fragment.bound);
            self.builder.copy_kids(chunk, start, bound, from, to);
            self.count += to - from;
        }
    }

    /// The fragment of the list, whose children take `size` positions.
    fn finish(self, size: usize) -> Fragment<'a> {
        if self.count == 0 {
            return Fragment::empty();
        }
        let size = u32::try_from(size).expect("a fragment smaller than 4G positions");
        Fragment::of(self.builder.seal(), self.start, self.count, size, u32::MAX)
    }
}

impl<'a> Fragment<'a> {
    pub fn empty() -> Fragment<'a> {
        Fragment {
            chunk: None,
            start: 0,
            count: 0,
            size: 0,
            bound: 0,
        }
    }

    #[inline]
    pub(crate) fn of(chunk: Arc<Chunk<'a>>, start: u32, count: u32, size: u32, bound: u32) -> Self {
        Fragment {
            chunk: Some(chunk),
            start,
            count,
            size,
            bound,
        }
    }

    #[inline]
    pub(crate) fn chunk(&self) -> Option<&Arc<Chunk<'a>>> {
        self.chunk.as_ref()
    }

    /// Where the list starts in its chunk's kids.
    #[inline]
    pub(crate) fn start(&self) -> u32 {
        self.start
    }

    /// A fragment of these nodes, as they are.
    pub(crate) fn new(nodes: &[Node<'a>]) -> Fragment<'a> {
        let Some(first) = nodes.first() else {
            return Fragment::empty();
        };
        let mut list = List::new(first.schema());
        for node in nodes {
            list.push(node);
        }
        list.finish(nodes.iter().map(Node::node_size).sum())
    }

    /// A fragment of these nodes, joining adjacent text nodes with the same marks.
    pub fn from_array(mut nodes: Vec<Node<'a>>) -> Fragment<'a> {
        nodes.dedup_by(|next, last| match last.join_text(next) {
            Some(joined) => {
                *last = joined;
                true
            }
            None => false,
        });
        Fragment::new(&nodes)
    }

    pub fn from_node(node: Node<'a>) -> Fragment<'a> {
        Fragment::new(&[node])
    }

    /// The schema of the chunk the fragment's list is in; `None` for no children.
    pub fn schema(&self) -> Option<&Schema> {
        self.chunk.as_ref().map(|chunk| chunk.schema())
    }

    /// An identity for the fragment, the same for clones of it.
    pub fn id(&self) -> (usize, u32, u32) {
        match &self.chunk {
            Some(chunk) if self.count > 0 => (
                Arc::as_ptr(chunk) as *const u8 as usize,
                self.start,
                self.count,
            ),
            _ => (0, 0, 0),
        }
    }

    /// Whether this is the very same fragment as `other`, not just an equal one. Every empty
    /// fragment is `Fragment.empty`.
    pub fn ptr_eq(&self, other: &Fragment) -> bool {
        self.id() == other.id()
    }

    pub fn size(&self) -> usize {
        self.size as usize
    }

    pub fn child_count(&self) -> usize {
        self.count as usize
    }

    /// The owner and index of child `index`, which must be one.
    #[inline]
    fn kid(&self, index: u32) -> (&Arc<Chunk<'a>>, u32) {
        let chunk = self
            .chunk
            .as_ref()
            .expect("a fragment with children is in a chunk");
        Chunk::child_shared(chunk, self.start, index, self.bound)
    }

    /// Child `index`, borrowed.
    #[inline]
    pub(crate) fn child_ref(&self, index: usize) -> NodeRef<'_> {
        let chunk = self
            .chunk
            .as_ref()
            .expect("a fragment with children is in a chunk");
        let (chunk, id) = chunk.child(self.start, index as u32, self.bound);
        NodeRef::at(chunk, id)
    }

    /// The size of child `index`, which the list holds with the child.
    #[inline]
    pub(crate) fn child_size(&self, index: u32) -> usize {
        let chunk = self
            .chunk
            .as_ref()
            .expect("a fragment with children is in a chunk");
        chunk.kid_size(self.start, index) as usize
    }

    /// The children, borrowed.
    pub(crate) fn refs(&self) -> impl DoubleEndedIterator<Item = NodeRef<'_>> + ExactSizeIterator {
        (0..self.child_count()).map(|index| self.child_ref(index))
    }

    #[inline]
    fn node(&self, index: u32) -> Node<'a> {
        let (chunk, id) = self.kid(index);
        Node::at(chunk.clone(), id)
    }

    pub fn children(&self) -> impl DoubleEndedIterator<Item = Node<'a>> + ExactSizeIterator + '_ {
        (0..self.count).map(|index| self.node(index))
    }

    /// The child at `index`, raising an error when there is none.
    pub fn child(&self, index: usize) -> Result<Node<'a>> {
        match self.maybe_child(index) {
            Some(child) => Ok(child),
            None => Err(Error::Range(format!(
                "Index {index} out of range for {}",
                self.to_debug_string()?
            ))),
        }
    }

    pub fn maybe_child(&self, index: usize) -> Option<Node<'a>> {
        (index < self.child_count()).then(|| self.node(index as u32))
    }

    pub fn first_child(&self) -> Option<Node<'a>> {
        self.maybe_child(0)
    }

    pub fn last_child(&self) -> Option<Node<'a>> {
        self.child_count()
            .checked_sub(1)
            .and_then(|last| self.maybe_child(last))
    }

    /// The children, each with its offset in the fragment.
    pub fn children_with_offsets(&self) -> impl Iterator<Item = (usize, Node<'a>)> + '_ {
        self.children().scan(0, |offset, child| {
            let at = *offset;
            *offset += child.node_size();
            Some((at, child))
        })
    }

    /// A ref to the fragment's list from a chunk being written, its length and its size: what
    /// a node written there whose content this is refers to.
    pub(crate) fn list(&self, builder: &mut Builder<'a>) -> (u32, u32, u32) {
        match &self.chunk {
            Some(chunk) if self.count > 0 => {
                (builder.external(chunk, self.start), self.count, self.size)
            }
            _ => (0, 0, 0),
        }
    }

    /// Call `f` for each node, at any depth, between `from` and `to`, counting positions from
    /// `node_start`.
    pub fn nodes_between(
        &self,
        from: usize,
        to: usize,
        f: &mut NodeVisitor<'_, 'a>,
        node_start: usize,
        parent: Option<&Node<'a>>,
    ) -> Result<()> {
        let mut pos = 0;
        for (index, child) in self.children().enumerate() {
            if pos >= to {
                break;
            }
            let end = pos + child.node_size();
            if end > from
                && f(&child, node_start + pos, parent, index)?
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

    pub fn descendants(&self, f: &mut NodeVisitor<'_, 'a>) -> Result<()> {
        self.nodes_between(0, self.size(), f, 0, None)
    }

    /// The text between `from` and `to`. See [`Node::text_between`].
    pub fn text_between(
        &self,
        from: usize,
        to: usize,
        block_separator: Option<&Text>,
        mut leaf_text: Option<&mut LeafTextHook<'_, 'a>>,
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
    pub fn append(&self, other: &Fragment<'a>) -> Fragment<'a> {
        if other.size == 0 {
            return self.clone();
        }
        if self.size == 0 {
            return other.clone();
        }
        let (last, count) = (self.count - 1, other.count);
        let mut list = List::new(other.schema().expect("a fragment with children"));
        list.extend(self, 0, last);
        match self
            .node(last)
            .join_text_into(&mut list.builder, &other.node(0))
        {
            Some(joined) => {
                list.push_kid(joined);
                list.extend(other, 1, count);
            }
            None => {
                list.extend(self, last, last + 1);
                list.extend(other, 0, count);
            }
        }
        list.finish(self.size() + other.size())
    }

    /// The part of the fragment between `from` and `to`.
    pub fn cut(&self, from: usize, to: usize) -> Fragment<'a> {
        if from == 0 && to == self.size() {
            return self.clone();
        }
        let Some(schema) = self.schema().filter(|_| to > from) else {
            return Fragment::empty();
        };
        let mut list = List::new(schema);
        let (mut pos, mut size) = (0, 0);
        // The children from `whole` to the one before `index` lie inside the cut, and aren't
        // listed yet.
        let (mut whole, mut index) = (0, 0);
        while index < self.count && pos < to {
            let child_size = self.child_size(index);
            let end = pos + child_size;
            if end <= from {
                whole = index + 1;
            } else if pos < from || end > to {
                list.extend(self, whole, index);
                let child = self.node(index);
                let (cut_from, cut_to) = (from.saturating_sub(pos), to - pos);
                match child.cut_text_into(&mut list.builder, cut_from, cut_to) {
                    Some(kid) => {
                        size += cut_to.min(child_size) - cut_from;
                        list.push_kid(kid);
                    }
                    None => {
                        let cut = stack::grow(|| {
                            let content_to = child.content().size().min(cut_to - 1);
                            child.cut_content(from.saturating_sub(pos + 1), content_to)
                        });
                        size += cut.node_size();
                        list.push(&cut);
                    }
                }
                whole = index + 1;
            } else {
                size += child_size;
            }
            pos = end;
            index += 1;
        }
        list.extend(self, whole, index);
        list.finish(size)
    }

    /// The children from index `from` to `to`: a part of the same list.
    pub fn cut_by_index(&self, from: usize, to: usize) -> Fragment<'a> {
        let to = to.min(self.child_count());
        let from = from.min(to);
        if from == to {
            return Fragment::empty();
        }
        if from == 0 && to == self.child_count() {
            return self.clone();
        }
        let size: usize = (from..to).map(|index| self.child_size(index as u32)).sum();
        Fragment {
            chunk: self.chunk.clone(),
            start: self.start + from as u32,
            count: (to - from) as u32,
            size: size as u32,
            bound: self.bound,
        }
    }

    /// The fragment with the child at `index` replaced by `node`.
    pub fn replace_child(&self, index: usize, node: Node<'a>) -> Fragment<'a> {
        let current = self.node(index as u32);
        if current.ptr_eq(&node) {
            return self.clone();
        }
        let index = index as u32;
        let mut list = List::new(node.schema());
        list.extend(self, 0, index);
        list.push(&node);
        list.extend(self, index + 1, self.count);
        list.finish(self.size() - current.node_size() + node.node_size())
    }

    pub fn add_to_start(&self, node: Node<'a>) -> Fragment<'a> {
        let mut list = List::new(node.schema());
        list.push(&node);
        list.extend(self, 0, self.count);
        list.finish(self.size() + node.node_size())
    }

    pub fn add_to_end(&self, node: Node<'a>) -> Fragment<'a> {
        let mut list = List::new(node.schema());
        list.extend(self, 0, self.count);
        list.push(&node);
        list.finish(self.size() + node.node_size())
    }

    /// The index of the child at `pos`, and that child's offset. At the end of a child, the
    /// index after it.
    pub fn find_index(&self, pos: usize) -> Result<(usize, usize)> {
        if pos == 0 {
            return Ok((0, pos));
        }
        if pos == self.size() {
            return Ok((self.child_count(), pos));
        }
        if pos > self.size() {
            return Err(Error::Range(format!(
                "Position {pos} outside of fragment ({})",
                self.to_debug_string()?
            )));
        }
        let mut offset = 0;
        for index in 0..self.child_count() {
            let end = offset + self.child_size(index as u32);
            if end >= pos {
                if end == pos {
                    return Ok((index + 1, end));
                }
                return Ok((index, offset));
            }
            offset = end;
        }
        Err(Error::Range(format!(
            "Position {pos} outside of fragment ({})",
            self.to_debug_string()?
        )))
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
        for (index, child) in self.children().enumerate() {
            if index > 0 {
                out.push_str(", ");
            }
            stack::grow(|| child.write_debug(out))?;
        }
        Ok(())
    }

    /// The children as JSON, or `null` when there are none.
    pub fn to_json(&self) -> Value {
        if self.count == 0 {
            return Value::Null;
        }
        Value::Array(self.refs().map(super::fields::node_value).collect())
    }

    pub fn from_json<'j>(schema: &Schema, json: impl Json<'j>) -> Result<Fragment<'static>> {
        let mut reader = Reader::new(schema);
        let (start, count, size) = reader.fragment_list(json)?;
        if count == 0 {
            return Ok(Fragment::empty());
        }
        Ok(Fragment::of(reader.finish(), start, count, size, u32::MAX))
    }
}

impl PartialEq for Fragment<'_> {
    fn eq(&self, other: &Fragment) -> bool {
        self.child_count() == other.child_count()
            && self.refs().zip(other.refs()).all(|(a, b)| a.equals(b))
    }
}

impl fmt::Debug for Fragment<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.to_debug_string() {
            Ok(described) => f.write_str(&described),
            Err(error) => write!(f, "<{error}>"),
        }
    }
}
