//! Nodes: the persistent tree a document is.

use std::fmt;
use std::sync::Arc;

use super::attrs::Attrs;
use super::content::ContentMatch;
use super::fragment::{Fragment, LeafTextHook, NodeVisitor};
use super::mark::{Mark, Marks};
use super::read::Reader;
use super::replace::{self, Slice};
use super::resolved_pos::ResolvedPos;
use super::schema::{MarkType, NodeType, Schema};
use super::view::{NodeRef, TextRef};
use crate::chunk::{Builder, Chunk, EXTERN, Record, TEXT_NODE, ValueRef, value_equals};
use crate::error::{Error, Result};
use crate::js::Json;
use crate::json::{self, Map};
use crate::stack;
use crate::text::Text;

/// A node of a document: a record in a chunk. Nodes are persistent: changing one writes a new
/// one, into a new chunk, sharing what it can with the old.
#[derive(Clone)]
pub struct Node<'a> {
    /// The node's children, a range of its chunk's kids, which come before it: the fragment's
    /// bound is the node's index.
    content: Fragment<'a>,
    record: Record,
}

/// A child found by [`Node::child_after`] or [`Node::child_before`]: the child, if there is one,
/// its index, and its offset in the parent.
pub struct ChildAt<'a> {
    pub node: Option<Node<'a>>,
    pub index: usize,
    pub offset: usize,
}

impl<'a> Node<'a> {
    /// The node at `id` in `chunk`.
    #[inline]
    pub(crate) fn at(chunk: Arc<Chunk<'a>>, id: u32) -> Node<'a> {
        let record = chunk.record(id);
        let content = if record.flags & TEXT_NODE != 0 {
            Fragment::of(chunk, 0, 0, 0, id)
        } else {
            Fragment::of(chunk, record.a, record.b, record.size, id)
        };
        Node { content, record }
    }

    /// The node a ref in `chunk` names.
    #[inline]
    pub(crate) fn at_ref(chunk: &Arc<Chunk<'a>>, reference: u32) -> Node<'a> {
        let (chunk, id) = Chunk::resolve_shared(chunk, reference);
        Node::at(chunk.clone(), id)
    }

    #[inline]
    pub(crate) fn chunk(&self) -> &Arc<Chunk<'a>> {
        self.content.chunk().expect("a node is in a chunk")
    }

    #[inline]
    pub(crate) fn index(&self) -> u32 {
        self.content.bound()
    }

    #[inline]
    pub(crate) fn view(&self) -> NodeRef<'_> {
        NodeRef {
            chunk: self.chunk(),
            id: self.index(),
            record: self.record,
        }
    }

    /// A node in a chunk of its own, written by `write`.
    pub(crate) fn build(schema: &Schema, write: impl FnOnce(&mut Builder<'a>) -> u32) -> Node<'a> {
        let mut builder = Builder::new(schema);
        let id = write(&mut builder);
        Node::at(builder.seal(), id)
    }

    /// A node of this type, with these attributes, content and marks.
    pub(crate) fn new(
        node_type: &NodeType,
        attrs: &Map,
        content: &Fragment<'a>,
        marks: &Marks<'a>,
    ) -> Node<'a> {
        Node::build(node_type.schema(), |builder| {
            let attrs = builder.map(attrs);
            let marks = marks.write(builder);
            let kids = content.write_kids(builder);
            builder.element(
                node_type.index() as u16,
                marks,
                attrs,
                &kids,
                content.size() as u32,
            )
        })
    }

    /// A text node, which may not be empty.
    pub(crate) fn new_text(schema: &Schema, text: &Text, marks: &Marks<'a>) -> Result<Node<'a>> {
        if text.is_empty() {
            return Err(Error::Range("Empty text nodes are not allowed".into()));
        }
        Ok(Node::build(schema, |builder| {
            let marks = marks.write(builder);
            builder.text_of(schema.text_type().index() as u16, marks, text)
        }))
    }

    /// This node's markup with other content: a node of the same type, attributes and marks.
    fn with_content(&self, content: &Fragment<'a>) -> Node<'a> {
        Node::build(self.chunk().schema(), |builder| {
            let marks = builder.reference(self.chunk(), self.record.marks);
            let attrs = builder.reference(self.chunk(), self.record.attrs);
            let kids = content.write_kids(builder);
            builder.element(self.record.ty, marks, attrs, &kids, content.size() as u32)
        })
    }

    pub fn node_type(&self) -> NodeType<'_> {
        self.view().node_type()
    }

    pub(crate) fn type_index(&self) -> usize {
        usize::from(self.record.ty)
    }

    pub fn schema(&self) -> &Schema {
        self.chunk().schema()
    }

    pub fn attrs(&self) -> Attrs<'a> {
        let (chunk, value) = Chunk::resolve_shared(self.chunk(), self.record.attrs);
        Attrs {
            chunk: chunk.clone(),
            value,
        }
    }

    /// The attributes, borrowed.
    pub fn attrs_view(&self) -> ValueRef<'_> {
        self.view().attrs()
    }

    pub fn content(&self) -> &Fragment<'a> {
        &self.content
    }

    pub fn marks(&self) -> Marks<'a> {
        Marks::at(self.chunk(), self.record.marks)
    }

    /// A text node's text.
    pub fn text(&self) -> Option<TextRef<'_>> {
        self.view().text()
    }

    /// Whether this is the very same node as `other`, not just an equal one.
    pub fn ptr_eq(&self, other: &Node) -> bool {
        self.index() == other.index() && self.chunk().ptr_eq(other.chunk())
    }

    /// An identity for the node, the same for clones of it: its chunk's and its index there.
    pub fn id(&self) -> (usize, u32) {
        (
            Arc::as_ptr(self.chunk()) as *const u8 as usize,
            self.index(),
        )
    }

    pub fn children(&self) -> impl DoubleEndedIterator<Item = Node<'a>> + ExactSizeIterator + '_ {
        self.content.children()
    }

    /// The size of the node in positions: a text node's length, 1 for another leaf, and for
    /// anything else its content's size plus its start and end tokens.
    pub fn node_size(&self) -> usize {
        self.view().node_size()
    }

    pub fn child_count(&self) -> usize {
        self.content.child_count()
    }

    pub fn child(&self, index: usize) -> Result<Node<'a>> {
        self.content.child(index)
    }

    pub fn maybe_child(&self, index: usize) -> Option<Node<'a>> {
        self.content.maybe_child(index)
    }

    pub fn first_child(&self) -> Option<Node<'a>> {
        self.content.first_child()
    }

    pub fn last_child(&self) -> Option<Node<'a>> {
        self.content.last_child()
    }

    /// Call `f` for each node, at any depth, that overlaps `from` to `to` in this node's
    /// content, counting positions from `start_pos`. See [`NodeVisitor`].
    pub fn nodes_between(
        &self,
        from: usize,
        to: usize,
        f: &mut NodeVisitor<'_, 'a>,
        start_pos: usize,
    ) -> Result<()> {
        self.content.nodes_between(from, to, f, start_pos, Some(self))
    }

    pub fn descendants(&self, f: &mut NodeVisitor<'_, 'a>) -> Result<()> {
        self.nodes_between(0, self.content.size(), f, 0)
    }

    /// All the text in the node: its own, its leaf's `leaf_text`, or its descendants'.
    pub fn text_content(&self) -> Result<Text> {
        if let Some(text) = self.text() {
            return Ok(text.to_text());
        }
        if self.is_leaf()
            && let Some(hook) = &self.node_type().spec().leaf_text
        {
            return hook(self);
        }
        self.text_between(0, self.content.size(), Some(&Text::default()), None)
    }

    /// The text between `from` and `to`, with `block_separator` between blocks' text, and the
    /// text of other leaves from `leaf_text`, or their spec's `leaf_text`.
    pub fn text_between(
        &self,
        from: usize,
        to: usize,
        block_separator: Option<&Text>,
        leaf_text: Option<&mut LeafTextHook<'_, 'a>>,
    ) -> Result<Text> {
        match self.text() {
            Some(text) => Ok(text.slice(from, to)),
            None => self
                .content
                .text_between(from, to, block_separator, leaf_text),
        }
    }

    /// Whether the nodes have the same markup and content.
    pub fn same_markup(&self, other: &Node) -> bool {
        self.view().same_markup(other.view())
    }

    /// Whether the node has this type, these attributes (by default the type's defaults) and
    /// these marks (by default none).
    pub fn has_markup(
        &self,
        node_type: &NodeType,
        attrs: Option<&Map>,
        marks: Option<&[Mark]>,
    ) -> bool {
        let attrs = attrs.unwrap_or_else(|| node_type.default_attrs().unwrap_or(&json::EMPTY));
        let own = self.view().marks();
        let marks = marks.unwrap_or(&[]);
        self.node_type() == *node_type
            && self.attrs_view().equals_map(attrs)
            && own.len() == marks.len()
            && own.iter().zip(marks).all(|(own, mark)| own.equals(mark.view()))
    }

    /// The node with this content, or itself when that is its content.
    pub fn copy(&self, content: Fragment<'a>) -> Node<'a> {
        if content.ptr_eq(&self.content) {
            return self.clone();
        }
        self.with_content(&content)
    }

    /// The node with these marks, or itself when they are its marks.
    pub fn mark(&self, marks: Marks<'a>) -> Node<'a> {
        if marks.ptr_eq(&self.marks()) {
            return self.clone();
        }
        Node::build(self.chunk().schema(), |builder| {
            let marks = marks.write(builder);
            let mut record = self.record;
            record.marks = marks;
            record.attrs = builder.reference(self.chunk(), self.record.attrs);
            if self.is_text() {
                record.a = builder.reference(self.chunk(), self.record.a);
                builder.text_record(record)
            } else {
                let kids = self.content.write_kids(builder);
                builder.element(record.ty, record.marks, record.attrs, &kids, record.size)
            }
        })
    }

    /// A text node with this text in place of its own, or itself when the text is the same.
    ///
    /// # Panics
    ///
    /// When this isn't a text node.
    pub fn with_text(&self, text: Text) -> Result<Node<'a>> {
        assert!(
            self.is_text(),
            "with_text on a {} node",
            self.node_type().name()
        );
        if text.is_empty() {
            return Err(Error::Range("Empty text nodes are not allowed".into()));
        }
        Ok(self.with_nonempty_text(&text))
    }

    pub(crate) fn with_nonempty_text(&self, text: &Text) -> Node<'a> {
        if self.text().is_some_and(|own| own == *text) {
            return self.clone();
        }
        Node::build(self.chunk().schema(), |builder| {
            let marks = builder.reference(self.chunk(), self.record.marks);
            builder.text_of(self.record.ty, marks, text)
        })
    }

    /// This text node's text from UTF-16 offset `from` to `to`, which mustn't be empty: its own
    /// bytes, where the cuts fall between characters.
    fn text_part(&self, text: TextRef, from: usize, to: usize) -> Node<'a> {
        if from == 0 && to == text.len() {
            return self.clone();
        }
        let Some((start, end)) = text.byte_range(from, to) else {
            return self.with_nonempty_text(&text.slice(from, to));
        };
        Node::build(self.chunk().schema(), |builder| {
            let mut record = self.record;
            record.marks = builder.reference(self.chunk(), self.record.marks);
            record.attrs = 0;
            let (chunk, offset) = Chunk::resolve_shared(self.chunk(), self.record.a);
            let chunk = chunk.clone();
            record.a = builder.external(&chunk, offset + start as u32);
            record.b = (end - start) as u32;
            record.size = (to - from) as u32;
            if record.flags & crate::chunk::ASCII == 0 && text.as_str().is_some_and(|text| text[start..end].is_ascii()) {
                record.flags |= crate::chunk::ASCII;
            }
            builder.text_record(record)
        })
    }

    /// The node with only its content between `from` and `to`; for a text node, only that part
    /// of its text, which may not be empty.
    pub fn cut(&self, from: usize, to: usize) -> Result<Node<'a>> {
        match self.text() {
            Some(text) => {
                let to = to.min(text.len());
                let from = from.min(to);
                if from == to {
                    return Err(Error::Range("Empty text nodes are not allowed".into()));
                }
                Ok(self.text_part(text, from, to))
            }
            None => Ok(self.cut_content(from, to)),
        }
    }

    pub(crate) fn cut_content(&self, from: usize, to: usize) -> Node<'a> {
        if from == 0 && to == self.content.size() {
            return self.clone();
        }
        self.copy(self.content.cut(from, to))
    }

    /// A text node with only its text from `from` to `to`, which mustn't be empty; `None` for
    /// a node that isn't text.
    pub(crate) fn cut_text(&self, from: usize, to: usize) -> Option<Node<'a>> {
        let text = self.text()?;
        let to = to.min(text.len());
        let from = from.min(to);
        Some(self.text_part(text, from, to))
    }

    /// This text node and `next` as one, when both are text with the same marks.
    pub(crate) fn join_text(&self, next: &Node<'a>) -> Option<Node<'a>> {
        let (text, more) = (self.text()?, next.text()?);
        if !self.same_markup(next) {
            return None;
        }
        Some(self.with_nonempty_text(&[&text.to_text(), &more.to_text()].into_iter().collect()))
    }

    /// The document between `from` and `to` as a slice. With `include_parents`, the slice is
    /// open all the way to this node.
    pub fn slice(&self, from: usize, to: usize, include_parents: bool) -> Result<Slice<'a>> {
        if from == to {
            return Ok(Slice::empty());
        }
        let resolved_from = self.resolve(from)?;
        let resolved_to = self.resolve(to)?;
        let depth = if include_parents {
            0
        } else {
            resolved_from.shared_depth(to)
        };
        let start = resolved_from.start(depth);
        let node = resolved_from.node(depth);
        let content = node
            .content()
            .cut(resolved_from.pos() - start, resolved_to.pos() - start);
        Ok(Slice::new(
            content,
            resolved_from.depth() - depth,
            resolved_to.depth() - depth,
        ))
    }

    /// The document with `from` to `to` replaced by `slice`, which must fit there.
    pub fn replace(&self, from: usize, to: usize, slice: &Slice<'a>) -> Result<Node<'a>> {
        replace::replace(&self.resolve(from)?, &self.resolve(to)?, slice)
    }

    /// [`replace`](Self::replace), on a document the caller gives up.
    pub fn into_replaced(self, from: usize, to: usize, slice: &Slice<'a>) -> Result<Node<'a>> {
        self.replace(from, to, slice)
    }

    /// The node directly after `pos`.
    pub fn node_at(&self, pos: usize) -> Result<Option<Node<'a>>> {
        let mut node = self.clone();
        let mut pos = pos;
        loop {
            let (index, offset) = node.content().find_index(pos)?;
            let Some(child) = node.maybe_child(index) else {
                return Ok(None);
            };
            if offset == pos || child.is_text() {
                return Ok(Some(child));
            }
            pos -= offset + 1;
            node = child;
        }
    }

    /// The child directly after `pos` in this node's content.
    pub fn child_after(&self, pos: usize) -> Result<ChildAt<'a>> {
        let (index, offset) = self.content().find_index(pos)?;
        Ok(ChildAt {
            node: self.maybe_child(index),
            index,
            offset,
        })
    }

    /// The child directly before `pos` in this node's content.
    pub fn child_before(&self, pos: usize) -> Result<ChildAt<'a>> {
        if pos == 0 {
            return Ok(ChildAt {
                node: None,
                index: 0,
                offset: 0,
            });
        }
        let (index, offset) = self.content().find_index(pos)?;
        if offset < pos {
            return Ok(ChildAt {
                node: Some(self.child(index)?),
                index,
                offset,
            });
        }
        let node = self.child(index - 1)?;
        let offset = offset - node.node_size();
        Ok(ChildAt {
            node: Some(node),
            index: index - 1,
            offset,
        })
    }

    pub fn resolve(&self, pos: usize) -> Result<ResolvedPos<'a>> {
        ResolvedPos::resolve(self, pos)
    }

    /// Whether a node between `from` and `to` has `mark`.
    pub fn range_has_mark(&self, from: usize, to: usize, mark: &Mark) -> Result<bool> {
        let mark = mark.view();
        self.range_has(from, to, |node| {
            node.marks().iter().any(|other| other.equals(mark))
        })
    }

    /// Whether a node between `from` and `to` has a mark of this type.
    pub fn range_has_mark_type(
        &self,
        from: usize,
        to: usize,
        mark_type: &MarkType,
    ) -> Result<bool> {
        self.range_has(from, to, |node| {
            node.marks()
                .iter()
                .any(|mark| mark.mark_type() == *mark_type)
        })
    }

    fn range_has(&self, from: usize, to: usize, has: impl Fn(NodeRef) -> bool) -> Result<bool> {
        let mut found = false;
        if to > from {
            self.nodes_between(
                from,
                to,
                &mut |node, _, _, _| {
                    if has(node.view()) {
                        found = true;
                    }
                    Ok(!found)
                },
                0,
            )?;
        }
        Ok(found)
    }

    pub fn is_block(&self) -> bool {
        self.node_type().is_block()
    }

    pub fn is_textblock(&self) -> bool {
        self.node_type().is_textblock()
    }

    pub fn inline_content(&self) -> bool {
        self.node_type().inline_content()
    }

    pub fn is_inline(&self) -> bool {
        self.node_type().is_inline()
    }

    pub fn is_text(&self) -> bool {
        self.record.flags & TEXT_NODE != 0
    }

    pub fn is_leaf(&self) -> bool {
        self.node_type().is_leaf()
    }

    pub fn is_atom(&self) -> bool {
        self.node_type().is_atom()
    }

    /// `toString`: the node described for debugging, by its spec's `to_debug_string` when it
    /// has one.
    pub fn to_debug_string(&self) -> Result<String> {
        let mut out = String::new();
        self.write_debug(&mut out)?;
        Ok(out)
    }

    pub(crate) fn write_debug(&self, out: &mut String) -> Result<()> {
        if let Some(hook) = &self.node_type().spec().to_debug_string {
            out.push_str(&hook(self)?);
            return Ok(());
        }
        let marks = self.view().marks();
        // Each mark wraps what the marks after it wrap.
        for mark in marks.iter() {
            out.push_str(mark.mark_type().name());
            out.push('(');
        }
        match self.text() {
            Some(text) => text.write_json(out),
            None => out.push_str(self.node_type().name()),
        }
        if self.content.size() > 0 {
            out.push('(');
            self.content.write_debug(out)?;
            out.push(')');
        }
        out.extend(marks.iter().map(|_| ')'));
        Ok(())
    }

    /// The content match after the children before `index`.
    pub fn content_match_at(&self, index: usize) -> Result<ContentMatch<'_>> {
        self.node_type()
            .content_match()
            .match_fragment_range(self.content(), 0, index)?
            .ok_or_else(|| {
                Error::Other("Called contentMatchAt on a node with invalid content".into())
            })
    }

    /// Whether replacing the children from index `from` to `to` with `replacement`'s from
    /// `start` to `end` leaves the content valid.
    pub fn can_replace(
        &self,
        from: usize,
        to: usize,
        replacement: &Fragment,
        start: usize,
        end: usize,
    ) -> Result<bool> {
        let one = self
            .content_match_at(from)?
            .match_fragment_range(replacement, start, end)?;
        let Some(one) = one else {
            return Ok(false);
        };
        let two = one.match_fragment_range(self.content(), to, self.child_count())?;
        if !two.is_some_and(|two| two.valid_end()) {
            return Ok(false);
        }
        for index in start..end {
            if !self
                .node_type()
                .allows_marks(&replacement.child(index)?.marks())
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Whether replacing the children from index `from` to `to` with a node of this type, with
    /// these marks, leaves the content valid.
    pub fn can_replace_with(
        &self,
        from: usize,
        to: usize,
        node_type: &NodeType,
        marks: Option<&[Mark]>,
    ) -> Result<bool> {
        if let Some(marks) = marks
            && !self.node_type().allows_mark_list(marks)
        {
            return Ok(false);
        }
        let Some(start) = self.content_match_at(from)?.match_type(node_type) else {
            return Ok(false);
        };
        let end = start.match_fragment_range(self.content(), to, self.child_count())?;
        Ok(end.is_some_and(|end| end.valid_end()))
    }

    /// Whether `other`'s content can be added to the end of this node's. An empty `other` can
    /// when some node could be in either.
    pub fn can_append(&self, other: &Node) -> Result<bool> {
        if other.content().size() > 0 {
            self.can_replace(
                self.child_count(),
                self.child_count(),
                other.content(),
                0,
                other.child_count(),
            )
        } else {
            Ok(self.node_type().compatible_content(&other.node_type()))
        }
    }

    /// Raise an error if this node or a descendant doesn't fit the schema.
    pub fn check(&self) -> Result<()> {
        let mut path = Vec::new();
        match check(self.view(), &mut path) {
            Ok(()) => Ok(()),
            Err(Failed::Error(error)) => Err(error),
            // The error describes the content, for which the schema's hooks need nodes.
            Err(Failed::Content) => {
                let mut node = self.clone();
                for index in path {
                    node = node.child(index as usize)?;
                }
                node.node_type().check_content(node.content())
            }
        }
    }

    pub fn from_json<'j>(schema: &Schema, json: impl Json<'j>) -> Result<Node<'static>> {
        let mut reader = Reader::new(schema);
        let id = reader.node(json)?;
        Ok(Node::at(reader.finish(), id))
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

/// `check`, over a node and its descendants, keeping the path of child indices to the node it's
/// at.
fn check(node: NodeRef, path: &mut Vec<u32>) -> Result<(), Failed> {
    let node_type = node.node_type();
    let data = node_type.data();
    let schema = node_type.schema();
    let kids = node
        .children()
        .map(|child| (child.chunk.schema() == schema).then(|| usize::from(child.record.ty)));
    let allowed = data.mark_set.is_none()
        || node
            .children()
            .all(|child| child.marks().iter().all(|mark| data.allows_mark(mark.rank())));
    if !(data.content.accepts(kids) && allowed) {
        return Err(Failed::Content);
    }
    data.attrs.check_ref(node.attrs(), "node", node_type.name())?;
    let marks = node.marks();
    for mark in marks.iter() {
        let mark_type = mark.mark_type();
        mark_type
            .data()
            .attrs
            .check_ref(mark.attrs(), "mark", mark_type.name())?;
    }
    // Adding one mark to no marks gives that mark, so only a longer set can be invalid.
    if marks.len() > 1 && !valid_set(marks) {
        let names: Vec<&str> = marks.iter().map(|mark| mark.mark_type().name()).collect();
        return Err(Failed::Error(Error::Range(format!(
            "Invalid collection of marks for node {}: {}",
            node_type.name(),
            names.join(",")
        ))));
    }
    for (index, child) in node.children().enumerate() {
        path.push(index as u32);
        stack::grow(|| check(child, path))?;
        path.pop();
    }
    Ok(())
}

/// Whether adding each mark in turn to no marks gives the set back: sorted by rank, with no
/// mark excluding another, and no mark twice.
fn valid_set(marks: super::view::SetRef) -> bool {
    let marks: Vec<_> = marks.iter().collect();
    let mut built: Vec<super::view::MarkRef> = Vec::with_capacity(marks.len());
    for &mark in &marks {
        let mark_type = mark.mark_type();
        let mut copy: Option<Vec<super::view::MarkRef>> = None;
        let mut placed = false;
        let mut kept = true;
        for (index, &other) in built.iter().enumerate() {
            if mark.equals(other) {
                kept = false;
                break;
            }
            let other_type = other.mark_type();
            if mark_type.excludes(&other_type) {
                copy.get_or_insert_with(|| built[..index].to_vec());
            } else if other_type.excludes(&mark_type) {
                kept = false;
                break;
            } else {
                if !placed && other_type.rank() > mark_type.rank() {
                    copy.get_or_insert_with(|| built[..index].to_vec()).push(mark);
                    placed = true;
                }
                if let Some(copy) = &mut copy {
                    copy.push(other);
                }
            }
        }
        if !kept {
            continue;
        }
        let mut next = copy.unwrap_or_else(|| built.clone());
        if !placed {
            next.push(mark);
        }
        built = next;
    }
    built.len() == marks.len() && built.iter().zip(&marks).all(|(a, b)| a.equals(*b))
}

impl PartialEq for Node<'_> {
    /// Whether the nodes are the same piece of document: the same markup and content.
    fn eq(&self, other: &Node) -> bool {
        self.view().equals(other.view())
    }
}

impl fmt::Debug for Node<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.to_debug_string() {
            Ok(described) => f.write_str(&described),
            Err(error) => write!(f, "<{error}>"),
        }
    }
}

