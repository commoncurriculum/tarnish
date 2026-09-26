//! Nodes: the persistent tree a document is.

use std::fmt;
use std::sync::Arc;

use super::compare_deep::objects_equal;
use super::content::ContentMatch;
use super::fragment::{Fragment, LeafTextHook, NodeVisitor};
use super::mark::{Mark, Marks};
use super::replace::{self, Slice};
use super::resolved_pos::ResolvedPos;
use super::schema::{Attrs, MarkType, NodeType, Schema};
use crate::error::{Error, Result};
use crate::js;
use crate::json::{Map, NULL, Value};
use crate::text::Text;

/// A node of a document. Nodes are persistent: changing one makes a new one, sharing what it
/// can with the old.
#[derive(Clone)]
pub struct Node(Arc<NodeData>);

struct NodeData {
    node_type: NodeType,
    attrs: Attrs,
    content: Fragment,
    marks: Marks,
    /// A text node's text, which is never empty. Other nodes have none.
    text: Option<Text>,
}

/// A child found by [`Node::child_after`] or [`Node::child_before`]: the child, if there is one,
/// its index, and its offset in the parent.
pub struct ChildAt<'a> {
    pub node: Option<&'a Node>,
    pub index: usize,
    pub offset: usize,
}

impl Node {
    pub(crate) fn new(node_type: NodeType, attrs: Attrs, content: Fragment, marks: Marks) -> Node {
        Node(Arc::new(NodeData {
            node_type,
            attrs,
            content,
            marks,
            text: None,
        }))
    }

    pub(crate) fn new_text(
        node_type: NodeType,
        attrs: Attrs,
        text: Text,
        marks: Marks,
    ) -> Result<Node> {
        if text.is_empty() {
            return Err(Error::Range("Empty text nodes are not allowed".into()));
        }
        Ok(Node(Arc::new(NodeData {
            node_type,
            attrs,
            content: Fragment::empty(),
            marks,
            text: Some(text),
        })))
    }

    pub fn node_type(&self) -> &NodeType {
        &self.0.node_type
    }

    pub fn attrs(&self) -> &Attrs {
        &self.0.attrs
    }

    pub fn content(&self) -> &Fragment {
        &self.0.content
    }

    pub fn marks(&self) -> &Marks {
        &self.0.marks
    }

    /// A text node's text.
    pub fn text(&self) -> Option<&Text> {
        self.0.text.as_ref()
    }

    /// Whether this is the very same node as `other`, not just an equal one.
    pub fn ptr_eq(&self, other: &Node) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// An identity for the node, the same for clones of it.
    pub fn id(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }

    pub fn children(&self) -> &[Node] {
        self.0.content.children()
    }

    /// The size of the node in positions: a text node's length, 1 for another leaf, and for
    /// anything else its content's size plus its start and end tokens.
    pub fn node_size(&self) -> usize {
        match &self.0.text {
            Some(text) => text.len(),
            None if self.is_leaf() => 1,
            None => 2 + self.0.content.size(),
        }
    }

    pub fn child_count(&self) -> usize {
        self.0.content.child_count()
    }

    pub fn child(&self, index: usize) -> Result<&Node> {
        self.0.content.child(index)
    }

    pub fn maybe_child(&self, index: usize) -> Option<&Node> {
        self.0.content.maybe_child(index)
    }

    pub fn first_child(&self) -> Option<&Node> {
        self.0.content.first_child()
    }

    pub fn last_child(&self) -> Option<&Node> {
        self.0.content.last_child()
    }

    /// Call `f` for each node, at any depth, that overlaps `from` to `to` in this node's
    /// content, counting positions from `start_pos`. See [`NodeVisitor`].
    pub fn nodes_between(
        &self,
        from: usize,
        to: usize,
        f: &mut NodeVisitor,
        start_pos: usize,
    ) -> Result<()> {
        self.0
            .content
            .nodes_between(from, to, f, start_pos, Some(self))
    }

    pub fn descendants(&self, f: &mut NodeVisitor) -> Result<()> {
        self.nodes_between(0, self.0.content.size(), f, 0)
    }

    /// All the text in the node: its own, its leaf's `leaf_text`, or its descendants'.
    pub fn text_content(&self) -> Result<Text> {
        if let Some(text) = &self.0.text {
            return Ok(text.clone());
        }
        if self.is_leaf()
            && let Some(hook) = &self.node_type().spec().leaf_text
        {
            return hook(self);
        }
        self.text_between(0, self.0.content.size(), Some(&Text::default()), None)
    }

    /// The text between `from` and `to`, with `block_separator` between blocks' text, and the
    /// text of other leaves from `leaf_text`, or their spec's `leaf_text`.
    pub fn text_between(
        &self,
        from: usize,
        to: usize,
        block_separator: Option<&Text>,
        leaf_text: Option<&mut LeafTextHook>,
    ) -> Result<Text> {
        match &self.0.text {
            Some(text) => Ok(text.slice(from, to)),
            None => self
                .0
                .content
                .text_between(from, to, block_separator, leaf_text),
        }
    }

    /// Whether the nodes have the same markup and content.
    pub fn same_markup(&self, other: &Node) -> bool {
        self.has_markup(other.node_type(), Some(other.attrs()), Some(other.marks()))
    }

    /// Whether the node has this type, these attributes (by default the type's defaults) and
    /// these marks (by default none).
    pub fn has_markup(
        &self,
        node_type: &NodeType,
        attrs: Option<&Map>,
        marks: Option<&[Mark]>,
    ) -> bool {
        static EMPTY: Map = Map::new();
        let attrs = attrs.unwrap_or_else(|| {
            node_type
                .default_attrs()
                .map_or(&EMPTY, |defaults| &**defaults)
        });
        self.node_type() == node_type
            && objects_equal(self.attrs(), attrs)
            && Mark::same_set(self.marks(), marks.unwrap_or(&[]))
    }

    /// The node with this content, or itself when that is its content.
    pub fn copy(&self, content: Fragment) -> Node {
        if content.ptr_eq(&self.0.content) {
            return self.clone();
        }
        Node(Arc::new(NodeData {
            node_type: self.0.node_type.clone(),
            attrs: self.0.attrs.clone(),
            content,
            marks: self.0.marks.clone(),
            text: self.0.text.clone(),
        }))
    }

    /// The node with these marks, or itself when they are its marks.
    pub fn mark(&self, marks: Marks) -> Node {
        if Arc::ptr_eq(&marks, &self.0.marks) {
            return self.clone();
        }
        Node(Arc::new(NodeData {
            node_type: self.0.node_type.clone(),
            attrs: self.0.attrs.clone(),
            content: self.0.content.clone(),
            marks,
            text: self.0.text.clone(),
        }))
    }

    /// A text node with this text in place of its own, or itself when the text is the same.
    ///
    /// # Panics
    ///
    /// When this isn't a text node.
    pub fn with_text(&self, text: Text) -> Result<Node> {
        assert!(
            self.is_text(),
            "with_text on a {} node",
            self.node_type().name()
        );
        if text.is_empty() {
            return Err(Error::Range("Empty text nodes are not allowed".into()));
        }
        Ok(self.with_nonempty_text(text))
    }

    pub(crate) fn with_nonempty_text(&self, text: Text) -> Node {
        if self.0.text.as_ref() == Some(&text) {
            return self.clone();
        }
        Node(Arc::new(NodeData {
            node_type: self.0.node_type.clone(),
            attrs: self.0.attrs.clone(),
            content: Fragment::empty(),
            marks: self.0.marks.clone(),
            text: Some(text),
        }))
    }

    /// The node with only its content between `from` and `to`; for a text node, only that part
    /// of its text, which may not be empty.
    pub fn cut(&self, from: usize, to: usize) -> Result<Node> {
        match &self.0.text {
            Some(text) => self.with_text(text.slice(from, to)),
            None => Ok(self.cut_content(from, to)),
        }
    }

    pub(crate) fn cut_content(&self, from: usize, to: usize) -> Node {
        if from == 0 && to == self.0.content.size() {
            return self.clone();
        }
        self.copy(self.0.content.cut(from, to))
    }

    /// Where a text node's cut from `from` to `to` isn't empty, the cut.
    pub(crate) fn cut_within(&self, from: usize, to: usize) -> Node {
        match &self.0.text {
            Some(text) => self.with_nonempty_text(text.slice(from, to)),
            None => self.cut_content(from, to),
        }
    }

    /// The document between `from` and `to` as a slice. With `include_parents`, the slice is
    /// open all the way to this node.
    pub fn slice(&self, from: usize, to: usize, include_parents: bool) -> Result<Slice> {
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
    pub fn replace(&self, from: usize, to: usize, slice: &Slice) -> Result<Node> {
        replace::replace(&self.resolve(from)?, &self.resolve(to)?, slice)
    }

    /// The node directly after `pos`.
    pub fn node_at(&self, pos: usize) -> Result<Option<&Node>> {
        let mut node = self;
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
    pub fn child_after(&self, pos: usize) -> Result<ChildAt<'_>> {
        let (index, offset) = self.content().find_index(pos)?;
        Ok(ChildAt {
            node: self.maybe_child(index),
            index,
            offset,
        })
    }

    /// The child directly before `pos` in this node's content.
    pub fn child_before(&self, pos: usize) -> Result<ChildAt<'_>> {
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
        Ok(ChildAt {
            node: Some(node),
            index: index - 1,
            offset: offset - node.node_size(),
        })
    }

    pub fn resolve(&self, pos: usize) -> Result<ResolvedPos> {
        ResolvedPos::resolve(self, pos)
    }

    /// Whether a node between `from` and `to` has `mark`.
    pub fn range_has_mark(&self, from: usize, to: usize, mark: &Mark) -> Result<bool> {
        self.range_has(from, to, |marks| mark.is_in_set(marks))
    }

    /// Whether a node between `from` and `to` has a mark of this type.
    pub fn range_has_mark_type(
        &self,
        from: usize,
        to: usize,
        mark_type: &MarkType,
    ) -> Result<bool> {
        self.range_has(from, to, |marks| mark_type.is_in_set(marks).is_some())
    }

    fn range_has(&self, from: usize, to: usize, has: impl Fn(&[Mark]) -> bool) -> Result<bool> {
        let mut found = false;
        if to > from {
            self.nodes_between(
                from,
                to,
                &mut |node, _, _, _| {
                    if has(node.marks()) {
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
        self.node_type().is_text()
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
        crate::stack::check()?;
        if let Some(hook) = &self.node_type().spec().to_debug_string {
            return hook(self);
        }
        let mut described = match &self.0.text {
            Some(text) => text.to_json_string(),
            None => self.node_type().name().to_owned(),
        };
        if self.0.content.size() > 0 {
            described = format!("{described}({})", self.0.content.to_string_inner()?);
        }
        for mark in self.0.marks.iter().rev() {
            described = format!("{}({described})", mark.mark_type().name());
        }
        Ok(described)
    }

    /// The content match after the child at `index`.
    pub fn content_match_at(&self, index: usize) -> Result<ContentMatch> {
        self.node_type()
            .content_match()
            .match_fragment(self.content(), 0, index)
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
            .match_fragment(replacement, start, end);
        let two = one.and_then(|one| one.match_fragment(self.content(), to, self.child_count()));
        if !two.is_some_and(|two| two.valid_end()) {
            return Ok(false);
        }
        let end = end.min(replacement.child_count());
        Ok(replacement.children()[start.min(end)..end]
            .iter()
            .all(|child| self.node_type().allows_marks(child.marks())))
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
            && !self.node_type().allows_marks(marks)
        {
            return Ok(false);
        }
        let start = self.content_match_at(from)?.match_type(node_type);
        let end =
            start.and_then(|start| start.match_fragment(self.content(), to, self.child_count()));
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
            Ok(self.node_type().compatible_content(other.node_type()))
        }
    }

    /// Raise an error if this node or a descendant doesn't fit the schema.
    pub fn check(&self) -> Result<()> {
        crate::stack::check()?;
        self.node_type().check_content(self.content())?;
        self.node_type().check_attrs(self.attrs())?;
        let mut copy = Mark::none();
        for mark in self.marks().iter() {
            mark.mark_type().check_attrs(mark.attrs())?;
            copy = mark.add_to_set(&copy);
        }
        if !Mark::same_set(&copy, self.marks()) {
            let names: Vec<&str> = self
                .marks()
                .iter()
                .map(|mark| mark.mark_type().name())
                .collect();
            return Err(Error::Range(format!(
                "Invalid collection of marks for node {}: {}",
                self.node_type().name(),
                names.join(",")
            )));
        }
        self.children().iter().try_for_each(Node::check)
    }

    pub fn to_json(&self) -> Value {
        let mut json = Map::with_capacity(5);
        json.push("type".into(), Value::String(self.node_type().name().into()));
        if !self.attrs().is_empty() {
            json.push("attrs".into(), Value::Object((**self.attrs()).clone()));
        }
        if self.content().size() > 0 {
            json.push("content".into(), self.content().to_json());
        }
        if !self.marks().is_empty() {
            json.push(
                "marks".into(),
                Value::Array(self.marks().iter().map(Mark::to_json).collect()),
            );
        }
        if let Some(text) = &self.0.text {
            json.push("text".into(), Value::String(text.to_string_lossy()));
        }
        Value::Object(json)
    }

    pub fn from_json(schema: &Schema, json: &Value) -> Result<Node> {
        crate::stack::check()?;
        if !js::truthy(Some(json)) {
            return Err(Error::Range("Invalid input for Node.fromJSON".into()));
        }
        let marks = match json.get("marks") {
            marks if !js::truthy(marks) => None,
            Some(Value::Array(marks)) => Some(
                marks
                    .iter()
                    .map(|mark| Mark::from_json(schema, mark))
                    .collect::<Result<Vec<_>>>()?,
            ),
            _ => return Err(Error::Range("Invalid mark data for Node.fromJSON".into())),
        };
        let marks = marks.as_deref().unwrap_or(&[]);
        let name = js::string(json.get("type"));
        if name == "text" {
            let Some(Value::String(text)) = json.get("text") else {
                return Err(Error::Range("Invalid text node in JSON".into()));
            };
            return schema.text(text.as_str(), marks);
        }
        let content = Fragment::from_json(schema, json.get("content").unwrap_or(&NULL))?;
        let node =
            schema
                .expect_node_type(&name)?
                .create(js::attrs(json.get("attrs")), content, marks)?;
        node.node_type().check_attrs(node.attrs())?;
        Ok(node)
    }
}

impl PartialEq for Node {
    /// Whether the nodes are the same piece of document: the same markup and content.
    fn eq(&self, other: &Node) -> bool {
        self.ptr_eq(other)
            || (self.same_markup(other)
                && self.0.text == other.0.text
                && self.0.content == other.0.content)
    }
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.to_debug_string().map_err(|_| fmt::Error)?)
    }
}
