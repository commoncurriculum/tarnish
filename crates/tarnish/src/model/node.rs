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
use crate::chunk::{
    ASCII, Builder, Chunk, EXTERN, Holder, Kid, NODES, Record, TEXT_NODE, ValueRef,
};
use crate::error::{Error, Result};
use crate::js::Json;
use crate::json::{self, Map};
use crate::text::Text;

/// A node of a document: a record in a chunk. Nodes are persistent: changing one writes a new
/// one, into a new chunk, sharing what it can with the old.
#[derive(Clone)]
pub struct Node<'a> {
    /// The node's children: a list in its own chunk, of nodes before it, or in an import.
    content: Fragment<'a>,
    record: Record,
    id: u32,
    /// The node's chunk, when its content isn't in it.
    own: Option<Arc<Chunk<'a>>>,
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
        let (content, own) = if record.flags & TEXT_NODE != 0 {
            (Fragment::of(chunk, 0, 0, 0, id), None)
        } else if record.a & EXTERN == 0 {
            (
                Fragment::of(chunk, record.a, record.b, record.size, id),
                None,
            )
        } else {
            let (list, start, bound) = chunk.kids_of(id, record.a);
            let content = Fragment::of(list.clone(), start, record.b, record.size, bound);
            (content, Some(chunk))
        };
        Node {
            content,
            record,
            id,
            own,
        }
    }

    /// The last node a chunk holds, which is the document in a chunk read from JSON, or written
    /// by [`flatten`](Self::flatten) or [`compact`](Self::compact).
    pub fn root(chunk: Arc<Chunk<'a>>) -> Option<Node<'a>> {
        let last = chunk.count(NODES).checked_sub(1)?;
        Some(Node::at(chunk, last))
    }

    /// The chunk that holds the node.
    #[inline]
    pub fn chunk(&self) -> &Arc<Chunk<'a>> {
        match &self.own {
            Some(own) => own,
            None => self.content.chunk().expect("a node is in a chunk"),
        }
    }

    #[inline]
    pub(crate) fn index(&self) -> u32 {
        self.id
    }

    /// The node as its chunk holds it, for a walk over it that counts no references.
    #[inline]
    pub fn view(&self) -> NodeRef<'_> {
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
            let (first, count, size) = content.list(builder);
            builder.element(node_type.index() as u16, marks, attrs, first, count, size)
        })
    }

    /// A text node, which may not be empty.
    pub(crate) fn new_text(schema: &Schema, text: &Text, marks: &Marks<'a>) -> Result<Node<'a>> {
        if text.is_empty() {
            return Err(empty_text());
        }
        Ok(Node::build(schema, |builder| {
            let marks = marks.write(builder);
            builder.text_of(schema.text_type().index() as u16, marks, text)
        }))
    }

    /// This node's markup with other content: a node of the same type, attributes and marks.
    fn with_content(&self, content: &Fragment<'a>) -> Node<'a> {
        Node::build(self.chunk().schema(), |builder| {
            let (first, count, size) = content.list(builder);
            self.write_copy(builder, first, count, size)
        })
    }

    /// Writes into `builder` a node with this one's markup, holding the `count` kids listed
    /// from `first`, a kids ref, whose content is `size`.
    pub(crate) fn write_copy(
        &self,
        builder: &mut Builder<'a>,
        first: u32,
        count: u32,
        size: u32,
    ) -> u32 {
        let marks = builder.reference_markup(self.chunk(), self.record.marks);
        let attrs = builder.reference_markup(self.chunk(), self.record.attrs);
        builder.element(self.record.ty, marks, attrs, first, count, size)
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
        let (chunk, value) = self.chunk().resolve(self.record.attrs);
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
        self.content
            .nodes_between(from, to, f, start_pos, Some(self))
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
            && own
                .iter()
                .zip(marks)
                .all(|(own, mark)| own.equals(mark.view()))
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
            let attrs = builder.reference_markup(self.chunk(), self.record.attrs);
            if self.is_text() {
                let a = builder.reference(self.chunk(), self.record.a);
                builder.text_record(Record {
                    marks,
                    attrs,
                    a,
                    ..self.record
                })
            } else {
                let (first, count, size) = self.content.list(builder);
                builder.element(self.record.ty, marks, attrs, first, count, size)
            }
        })
    }

    /// A text node with this text in place of its own, or itself when the text is the same.
    ///
    /// # Panics
    ///
    /// When this isn't a text node.
    pub fn with_text(&self, text: Text) -> Result<Node<'a>> {
        let Some(own) = self.text() else {
            panic!("with_text on a {} node", self.node_type().name());
        };
        if text.is_empty() {
            return Err(empty_text());
        }
        if own == text {
            return Ok(self.clone());
        }
        Ok(Node::build(self.schema(), |builder| {
            let marks = builder.reference_markup(self.chunk(), self.record.marks);
            builder.text_of(self.record.ty, marks, &text)
        }))
    }

    /// A text node's text, and `from` and `to` as UTF-16 offsets clamped to it.
    fn text_range(&self, from: usize, to: usize) -> Option<(TextRef<'_>, usize, usize)> {
        let text = self.text()?;
        let to = to.min(text.len());
        Some((text, from.min(to), to))
    }

    /// This text node's text from UTF-16 offset `from` to `to`, which mustn't be empty: its own
    /// bytes, where the cuts fall between characters.
    fn text_part(&self, text: TextRef, from: usize, to: usize) -> Node<'a> {
        if from == 0 && to == text.len() {
            return self.clone();
        }
        Node::build(self.chunk().schema(), |builder| {
            self.write_text_part(builder, text, from, to)
        })
    }

    /// Writes [`text_part`](Self::text_part) into `builder`: its index.
    fn write_text_part(
        &self,
        builder: &mut Builder<'a>,
        text: TextRef,
        from: usize,
        to: usize,
    ) -> u32 {
        let marks = builder.reference_markup(self.chunk(), self.record.marks);
        let Some((start, end)) = text.byte_range(from, to) else {
            return builder.text_of(self.record.ty, marks, &text.slice(from, to));
        };
        let (chunk, offset) = self.chunk().resolve(self.record.a);
        let ascii = self.record.flags & ASCII != 0
            || text
                .as_str()
                .is_some_and(|text| text[start..end].is_ascii());
        let a = builder.external(chunk, offset + start as u32);
        builder.text_record(Record {
            flags: if ascii { TEXT_NODE | ASCII } else { TEXT_NODE },
            marks,
            attrs: 0,
            a,
            b: (end - start) as u32,
            size: (to - from) as u32,
            ..self.record
        })
    }

    /// A kid in `builder` for this text node with only its text from `from` to `to`, which
    /// mustn't be empty: the node itself, or one written there. `None` for a node that isn't
    /// text.
    pub(crate) fn cut_text_into(
        &self,
        builder: &mut Builder<'a>,
        from: usize,
        to: usize,
    ) -> Option<Kid> {
        let (text, from, to) = self.text_range(from, to)?;
        Some(match from == 0 && to == text.len() {
            true => builder.kid(self.chunk(), self.id, to),
            false => {
                let part = self.write_text_part(builder, text, from, to);
                Kid::local(part, (to - from) as u32)
            }
        })
    }

    /// Writes into `builder` one text node of these parts of text nodes with the same markup,
    /// each from and to a UTF-16 offset, with the first's markup.
    pub(crate) fn join_texts_into(
        builder: &mut Builder<'a>,
        parts: &[(Node<'a>, usize, usize)],
    ) -> Kid {
        let (first, ..) = &parts[0];
        let (ty, marks) = (
            first.record.ty,
            builder.reference_markup(first.chunk(), first.record.marks),
        );
        let texts: Option<Vec<TextRef>> = parts
            .iter()
            .map(|(node, from, to)| node.text()?.part(*from, *to))
            .collect();
        let (joined, size) = match texts {
            Some(texts) => (
                builder.text_of_parts(ty, marks, &texts),
                texts.iter().map(|text| text.len()).sum(),
            ),
            // A part splits a surrogate pair.
            None => {
                let texts: Vec<Text> = parts
                    .iter()
                    .map(|(node, from, to)| node.text().expect("text").slice(*from, *to))
                    .collect();
                let text: Text = texts.iter().collect();
                (builder.text_of(ty, marks, &text), text.len())
            }
        };
        Kid::local(joined, size as u32)
    }

    /// The node with only its content between `from` and `to`; for a text node, only that part
    /// of its text, which may not be empty.
    pub fn cut(&self, from: usize, to: usize) -> Result<Node<'a>> {
        match self.text_range(from, to) {
            Some((_, from, to)) if from == to => Err(empty_text()),
            Some((text, from, to)) => Ok(self.text_part(text, from, to)),
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
        let (text, from, to) = self.text_range(from, to)?;
        Some(self.text_part(text, from, to))
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
        self.clone().into_replaced(from, to, slice)
    }

    /// [`replace`](Self::replace), on a document the caller gives up. When the range lies inside
    /// one of its children, and nothing else holds the document or the list of its children,
    /// the new child takes the old one's place in the list, which isn't copied.
    pub fn into_replaced(self, from: usize, to: usize, slice: &Slice<'a>) -> Result<Node<'a>> {
        // The positions hold the node, which must be held by nothing else to change in place.
        let replaced = {
            let start = self.resolve(from)?;
            let end = if to == from {
                None
            } else {
                Some(self.resolve(to)?)
            };
            replace::replace_top(&start, end.as_ref().unwrap_or(&start), slice)?
        };
        Ok(match replaced {
            replace::Replaced::Node(node) => node,
            replace::Replaced::Child(index, child) => self.with_child(index as u32, child),
        })
    }

    /// The node with its child at `index` replaced: in place, when the node's record is in a
    /// chunk only the node holds, and its list in one only that chunk holds.
    fn with_child(self, index: u32, child: Node<'a>) -> Node<'a> {
        let Node {
            content,
            record,
            id,
            own,
        } = self;
        let Some(mut chunk) = own else {
            let node = Node {
                content,
                record,
                id,
                own: None,
            };
            return node.copy(node.content.replace_child(index as usize, child));
        };
        let (start, old_size) = (content.start(), content.child_size(index));
        drop(content);
        let size = record.size as usize - old_size + child.node_size();
        if set_child(&mut chunk, record.a, start, index, &child).is_some() {
            Arc::get_mut(&mut chunk)
                .expect("a chunk changed in place")
                .set_size(id, size as u32);
            return Node::at(chunk, id);
        }
        let node = Node::at(chunk, id);
        node.copy(node.content.replace_child(index as usize, child))
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

    pub fn from_json<'j>(schema: &Schema, json: impl Json<'j>) -> Result<Node<'static>> {
        let mut reader = Reader::new(schema);
        let id = reader.node(json)?;
        Ok(Node::at(reader.finish(), id))
    }
}

/// Puts `child` in place of kid `position` of the list at `start` that `kids`, a ref in `chunk`,
/// names in an import: `None`, having changed nothing, unless `chunk` and that import are held
/// by nothing else and own their bytes.
fn set_child<'a>(
    chunk: &mut Arc<Chunk<'a>>,
    kids: u32,
    start: u32,
    position: u32,
    child: &Node<'a>,
) -> Option<()> {
    let chunk = Arc::get_mut(chunk).filter(|chunk| chunk.is_owned())?;
    let (slot, _) = chunk.external_ref(kids)?;
    let list = chunk.import_mut(slot).filter(|list| list.is_owned())?;
    let child_slot = list.import_in_place(child.chunk())?;
    let kid = Kid {
        slot: child_slot,
        index: child.id,
        size: child.node_size() as u32,
    };
    list.set_kid(start, position, kid);
    Some(())
}

fn empty_text() -> Error {
    Error::Range("Empty text nodes are not allowed".into())
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
