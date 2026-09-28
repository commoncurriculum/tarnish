//! Fragments: a node's children.

use std::fmt;
use std::sync::Arc;

use super::node::Node;
use super::read::Reader;
use super::schema::Schema;
use super::view::NodeRef;
use crate::chunk::{Builder, Chunk, Holder, Kid};
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
    /// The positions the kids listed take.
    size: u64,
    /// Text held back from the list, to join to text with the same markup that comes next: the
    /// text nodes it's parts of, each from and to a UTF-16 offset.
    tail: Vec<(Node<'a>, usize, usize)>,
}

impl<'a> List<'a> {
    fn new(schema: &Schema) -> List<'a> {
        let builder = Builder::new(schema);
        let start = builder.kids_end();
        List {
            builder,
            start,
            count: 0,
            size: 0,
            tail: Vec::new(),
        }
    }

    /// Adds the children of `fragment` between `from` and `to`, which mustn't pass its end, cut
    /// as [`Fragment::cut`] cuts them, the first joined to the text held back as
    /// [`Fragment::append`] joins them. Text that comes last is held back in turn.
    fn add_cut(&mut self, fragment: &Fragment<'a>, from: usize, to: usize) {
        if from >= to {
            return;
        }
        let (mut first, mut start) = (0, 0);
        while start + fragment.child_size(first) <= from {
            start += fragment.child_size(first);
            first += 1;
        }
        let (mut last, mut end) = (first, start + fragment.child_size(first));
        while end < to {
            last += 1;
            end += fragment.child_size(last);
        }
        self.add_child(fragment.node(first), from.saturating_sub(start), to - start);
        if last > first {
            self.extend(fragment, first + 1, last);
            let last_start = end - fragment.child_size(last);
            self.add_child(fragment.node(last), 0, to - last_start);
        }
    }

    /// Adds `child` with only its content, or text, from `from` to `to`. Text is held back,
    /// joined to the text held back already when it has the same markup.
    fn add_child(&mut self, child: Node<'a>, from: usize, to: usize) {
        let to = to.min(child.node_size());
        if child.is_text() {
            if !self
                .tail
                .first()
                .is_some_and(|(text, ..)| text.same_markup(&child))
            {
                self.flush();
            }
            self.tail.push((child, from, to));
        } else if from == 0 && to == child.node_size() {
            self.flush();
            self.push(&child);
        } else {
            self.flush();
            let cut = stack::grow(|| {
                child.cut_content(from.saturating_sub(1), child.content().size().min(to - 1))
            });
            self.push(&cut);
        }
    }

    /// Lists the text held back.
    fn flush(&mut self) {
        let kid = match self.tail.as_slice() {
            [] => return,
            [(text, from, to)] => text
                .cut_text_into(&mut self.builder, *from, *to)
                .expect("text"),
            parts => Node::join_texts_into(&mut self.builder, parts),
        };
        self.tail.clear();
        self.push_kid(kid);
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
        self.size += u64::from(kid.size);
    }

    /// Adds `fragment`'s children from index `from` to `to`.
    fn extend(&mut self, fragment: &Fragment<'a>, from: u32, to: u32) {
        self.flush();
        if let Some(chunk) = &fragment.chunk
            && from < to
        {
            let (start, bound) = (fragment.start, fragment.bound);
            self.size += self.builder.copy_kids(chunk, start, bound, from, to);
            self.count += to - from;
        }
    }

    fn size(&self) -> u32 {
        u32::try_from(self.size).expect("a fragment smaller than 4G positions")
    }

    fn finish(mut self) -> Fragment<'a> {
        self.flush();
        if self.count == 0 {
            return Fragment::empty();
        }
        let size = self.size();
        Fragment::of(self.builder.seal(), self.start, self.count, size, u32::MAX)
    }

    /// A node with `node`'s markup holding the list, written with it.
    fn finish_copy(mut self, node: &Node<'a>) -> Node<'a> {
        self.flush();
        let first = if self.count == 0 { 0 } else { self.start };
        let (count, size) = (self.count, self.size());
        let id = node.write_copy(&mut self.builder, first, count, size);
        Node::at(self.builder.seal(), id)
    }
}

/// `content.cut(0, from).append(insert).append(content.cut(to))`: one of the three when the
/// others are empty, as `append` gives back what's appended to nothing, or a list of all three.
enum Replaced<'a> {
    Piece(Fragment<'a>),
    List(List<'a>),
}

fn replaced<'a>(
    content: &Fragment<'a>,
    from: usize,
    to: usize,
    insert: &Fragment<'a>,
) -> Replaced<'a> {
    let end = content.size();
    Replaced::Piece(match (from > 0, insert.size() > 0, to < end) {
        (true, false, false) => content.cut(0, from),
        (false, true, false) => insert.clone(),
        (false, false, true) => content.cut(to, end),
        (false, false, false) => Fragment::empty(),
        _ => {
            let schema = content.schema().expect("a fragment with children");
            let mut list = List::new(schema);
            list.add_cut(content, 0, from);
            list.add_cut(insert, 0, insert.size());
            list.add_cut(content, to, end);
            return Replaced::List(list);
        }
    })
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

    /// A fragment of these nodes, joining adjacent text nodes with the same marks.
    pub fn from_array(nodes: Vec<Node<'a>>) -> Fragment<'a> {
        let Some(first) = nodes.first() else {
            return Fragment::empty();
        };
        let mut list = List::new(first.schema());
        for node in nodes {
            let size = node.node_size();
            list.add_child(node, 0, size);
        }
        list.finish()
    }

    pub fn from_node(node: Node<'a>) -> Fragment<'a> {
        let mut list = List::new(node.schema());
        list.push(&node);
        list.finish()
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

    /// The chunk the list is in, which a fragment with children has.
    #[inline]
    fn list_chunk(&self) -> &Arc<Chunk<'a>> {
        self.chunk
            .as_ref()
            .expect("a fragment with children is in a chunk")
    }

    /// Child `index`, borrowed.
    #[inline]
    pub(crate) fn child_ref(&self, index: usize) -> NodeRef<'_> {
        let (chunk, id) = self
            .list_chunk()
            .child(self.start, index as u32, self.bound);
        NodeRef::at(chunk, id)
    }

    /// The size of child `index`, which the list holds with the child.
    #[inline]
    pub(crate) fn child_size(&self, index: u32) -> usize {
        self.list_chunk().kid_size(self.start, index) as usize
    }

    /// The children, borrowed.
    pub(crate) fn refs(
        &self,
    ) -> impl DoubleEndedIterator<Item = NodeRef<'_>> + ExactSizeIterator + Clone {
        (0..self.child_count()).map(|index| self.child_ref(index))
    }

    #[inline]
    fn node(&self, index: u32) -> Node<'a> {
        let (chunk, id) = self.list_chunk().child(self.start, index, self.bound);
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
        let (before, after) = (self.node(last), other.node(0));
        let (before_size, after_size) = (before.node_size(), after.node_size());
        list.add_child(before, 0, before_size);
        list.add_child(after, 0, after_size);
        list.extend(other, 1, count);
        list.finish()
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
        list.add_cut(self, from, to.min(self.size()));
        list.finish()
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

    /// `this.cut(0, from).append(insert).append(this.cut(to))`, written as one list.
    pub(crate) fn replace_range(
        &self,
        from: usize,
        to: usize,
        insert: &Fragment<'a>,
    ) -> Fragment<'a> {
        match replaced(self, from, to, insert) {
            Replaced::Piece(piece) => piece,
            Replaced::List(list) => list.finish(),
        }
    }

    /// `node.copy(this.cut(0, from).append(insert).append(this.cut(to)))`, this being the
    /// node's content, written with its list.
    pub(crate) fn copy_replaced(
        &self,
        node: &Node<'a>,
        from: usize,
        to: usize,
        insert: &Fragment<'a>,
    ) -> Node<'a> {
        match replaced(self, from, to, insert) {
            Replaced::Piece(piece) => node.copy(piece),
            Replaced::List(list) => list.finish_copy(node),
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
        list.finish()
    }

    pub fn add_to_start(&self, node: Node<'a>) -> Fragment<'a> {
        let mut list = List::new(node.schema());
        list.push(&node);
        list.extend(self, 0, self.count);
        list.finish()
    }

    pub fn add_to_end(&self, node: Node<'a>) -> Fragment<'a> {
        let mut list = List::new(node.schema());
        list.extend(self, 0, self.count);
        list.push(&node);
        list.finish()
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
        let kids = self.list_chunk().kid_list(self.start, self.count);
        let mut offset = 0;
        for (index, kid) in kids.enumerate() {
            let end = offset + kid.size as usize;
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
