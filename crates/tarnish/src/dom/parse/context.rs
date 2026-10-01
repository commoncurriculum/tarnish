//! The stack of nodes being parsed into, and fitting nodes into it.

use super::DomParser;
use super::node_context::{Item, NodeContext, WsOptions, written_node};
use super::{ParseOptions, PreserveWhitespace};
use crate::Result;
use crate::chunk::Builder;
use crate::dom::Dom;
use crate::js::deadline::Deadline;
use crate::js::text::Text;
use crate::json::Map;
use crate::model::{ContentMatch, Fragment, Mark, MarkType, Node, NodeType, Schema};

/// A node to insert where it fits: text, a leaf a rule makes, with the attributes it gives, or
/// a node made elsewhere.
pub(super) enum Inserted<'p> {
    Text(Text),
    Leaf(NodeType<'p>, Option<Map>),
    Node(Node<'static>),
}

/// `mark.addToSet(set)`, on a set held as a list.
fn add_to_set(set: &mut Vec<Mark<'static>>, mark: &Mark<'static>) {
    if let Some(added) = mark.added_to(set) {
        *set = added;
    }
}

pub(super) struct ParseContext<'p, D: Dom> {
    pub(super) parser: &'p DomParser<D::Node>,
    pub(super) dom: &'p D,
    pub(super) options: ParseOptions<'p, D::Node>,
    pub(super) is_open: bool,
    /// The index of the context content goes into. Those above it wait to be closed.
    pub(super) open: usize,
    pub(super) needs_block: bool,
    pub(super) nodes: Vec<NodeContext<'p>>,
    pub(super) local_preserve_ws: bool,
    next_id: usize,
    /// Each DOM node added is a turn, which can scan the marks and the nodes open.
    pub(super) deadline: Deadline,
    /// The chunk the parse writes each node into as it finishes it.
    builder: Builder<'static>,
}

impl<'p, D: Dom> ParseContext<'p, D> {
    pub(super) fn new(
        parser: &'p DomParser<D::Node>,
        dom: &'p D,
        options: ParseOptions<'p, D::Node>,
        is_open: bool,
    ) -> Self {
        let ws = WsOptions {
            preserve: options
                .preserve_whitespace
                .unwrap_or(PreserveWhitespace::No),
            open_left: is_open,
        };
        let top = match &options.top_node {
            Some(top_node) => {
                let node_type = parser.schema.node_type_at(top_node.node_type().index());
                let matched = match options.top_match {
                    Some(top_match) => top_match,
                    None => node_type.content_match(),
                };
                let attrs = Some(top_node.attrs().to_map());
                NodeContext::new(
                    0,
                    Some(node_type),
                    attrs,
                    Vec::new(),
                    true,
                    Some(matched),
                    ws,
                )
            }
            None => {
                let node_type = (!is_open).then(|| parser.schema.top_node_type());
                NodeContext::new(0, node_type, None, Vec::new(), true, None, ws)
            }
        };
        ParseContext {
            parser,
            dom,
            options,
            is_open,
            open: 0,
            needs_block: false,
            nodes: vec![top],
            local_preserve_ws: false,
            next_id: 1,
            deadline: Deadline::current(),
            builder: Builder::new(&parser.schema),
        }
    }

    pub(super) fn top(&self) -> &NodeContext<'p> {
        &self.nodes[self.open]
    }

    pub(super) fn top_mut(&mut self) -> &mut NodeContext<'p> {
        &mut self.nodes[self.open]
    }

    pub(super) fn schema(&self) -> &'p Schema {
        &self.parser.schema
    }

    /// Find a place for a node of this type: the context to put it in, leaving contexts that
    /// aren't solid and opening wrappers where needed. The marks the node gets, or `None` when
    /// it fits nowhere.
    pub(super) fn find_place(
        &mut self,
        node_type: &NodeType,
        marks: Vec<Mark<'static>>,
        cautious: bool,
    ) -> Result<Option<Vec<Mark<'static>>>> {
        let mut route: Option<(Vec<NodeType<'p>>, usize)> = None;
        let mut penalty = 0;
        for depth in (0..=self.open).rev() {
            let context = &mut self.nodes[depth];
            let found = context.find_wrapping(node_type)?;
            if let Some(found) = found
                && route
                    .as_ref()
                    .is_none_or(|(route, _)| route.len() > found.len() + penalty)
            {
                let done = found.is_empty();
                route = Some((found, context.id));
                if done {
                    break;
                }
            }
            if context.solid {
                if cautious {
                    break;
                }
                penalty += 2;
            }
        }
        let Some((route, sync)) = route else {
            return Ok(None);
        };
        self.sync(sync);
        let mut marks = marks;
        for node_type in route {
            marks = self.enter_inner(&node_type, None, marks, false, None)?;
        }
        Ok(Some(marks))
    }

    /// Insert a node, adjusting the context where it needs to. Whether it fit.
    pub(super) fn insert_node(
        &mut self,
        node: Inserted<'p>,
        marks: &[Mark<'static>],
        cautious: bool,
    ) -> Result<bool> {
        let (node_type, own_marks) = match &node {
            Inserted::Text(_) => (self.schema().text_type(), Vec::new()),
            Inserted::Leaf(node_type, _) => (node_type.clone(), Vec::new()),
            Inserted::Node(node) => (
                self.schema().node_type_at(node.node_type().index()),
                node.marks().to_vec(),
            ),
        };
        let mut marks = marks.to_vec();
        if node_type.is_inline()
            && self.needs_block
            && self.top().node_type.is_none()
            && let Some(block) = self.textblock_from_context()?
        {
            marks = self.enter_inner(&block, None, marks, false, None)?;
        }
        let Some(inner_marks) = self.find_place(&node_type, marks, cautious)? else {
            return Ok(false);
        };
        self.close_extra(false)?;
        let schema = self.schema();
        let top = self.top_mut();
        if let Some(matched) = &top.matched {
            top.matched = matched.match_type(&node_type);
        }
        let mut node_marks = Vec::new();
        for mark in inner_marks.iter().chain(&own_marks) {
            let applies = match &top.node_type {
                Some(parent) => parent.allows_mark_type(&mark.mark_type()),
                None => mark_may_apply(schema, &mark.mark_type(), &node_type),
            };
            if applies {
                add_to_set(&mut node_marks, mark);
            }
        }
        top.content.push(match node {
            Inserted::Text(text) => Item::Text(text, node_marks),
            Inserted::Leaf(node_type, attrs) => Item::Leaf(node_type, attrs, node_marks),
            Inserted::Node(node) => match node.text() {
                Some(text) => Item::Text(text.to_text(), node_marks),
                None => Item::Node(node.mark(Mark::set_from(&node_marks))),
            },
        });
        Ok(true)
    }

    /// Start a node of this type, adjusting the context where it needs to. The marks left for
    /// its content, or `None` when it fits nowhere.
    pub(super) fn enter(
        &mut self,
        node_type: &NodeType<'p>,
        attrs: Option<Map>,
        marks: Vec<Mark<'static>>,
        preserve: Option<PreserveWhitespace>,
    ) -> Result<Option<Vec<Mark<'static>>>> {
        node_type.check_create(attrs.as_ref())?;
        if self.find_place(node_type, marks.clone(), false)?.is_none() {
            return Ok(None);
        }
        self.enter_inner(node_type, attrs, marks, true, preserve)
            .map(Some)
    }

    /// Open a node of this type, giving it the marks it allows and leaving the rest.
    fn enter_inner(
        &mut self,
        node_type: &NodeType<'p>,
        attrs: Option<Map>,
        marks: Vec<Mark<'static>>,
        solid: bool,
        preserve: Option<PreserveWhitespace>,
    ) -> Result<Vec<Mark<'static>>> {
        self.close_extra(false)?;
        let schema = self.schema();
        let top = self.top_mut();
        top.matched = top
            .matched
            .as_ref()
            .and_then(|matched| matched.match_type(node_type));
        let ws = top.ws.child(node_type, preserve, top.content.is_empty());
        let mut apply = Vec::new();
        let mut rest = Vec::new();
        for mark in marks {
            let applies = match &top.node_type {
                Some(parent) => parent.allows_mark_type(&mark.mark_type()),
                None => mark_may_apply(schema, &mark.mark_type(), node_type),
            };
            if applies {
                add_to_set(&mut apply, &mark);
            } else {
                rest.push(mark);
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        let context = NodeContext::new(id, Some(node_type.clone()), attrs, apply, solid, None, ws);
        self.nodes.push(context);
        self.open += 1;
        Ok(rest)
    }

    /// Finish the nodes above the open one and add them to their parents.
    fn close_extra(&mut self, open_end: bool) -> Result<()> {
        while self.nodes.len() - 1 > self.open {
            let context = self.nodes.pop().expect("a node above the open one");
            let item = context.finish_node(open_end, &mut self.builder)?;
            self.nodes.last_mut().expect("a parent").content.push(item);
        }
        Ok(())
    }

    /// Close every node but the top one, and give it back with the chunk it is written into.
    fn finish(mut self) -> Result<(NodeContext<'p>, Builder<'static>)> {
        self.open = 0;
        self.close_extra(self.is_open)?;
        Ok((self.nodes.swap_remove(0), self.builder))
    }

    /// Finish the parse, as the top node.
    pub(super) fn finish_node(self, open_end: bool) -> Result<Node<'static>> {
        let (top, mut builder) = self.finish()?;
        let item = top.finish_node(open_end, &mut builder)?;
        Ok(written_node(builder, item))
    }

    /// Finish an open parse, as the content parsed into its top.
    pub(super) fn finish_content(self, open_end: bool) -> Result<Fragment<'static>> {
        let (top, builder) = self.finish()?;
        top.finish_content(open_end, builder)
    }

    /// Make the context with this identity the open one, if it is still on the stack.
    pub(super) fn sync(&mut self, to: usize) -> bool {
        for index in (0..=self.open).rev() {
            let context = &mut self.nodes[index];
            if context.id == to {
                self.open = index;
                return true;
            } else if self.local_preserve_ws {
                context.ws.preserve = context.ws.preserve.max(PreserveWhitespace::Yes);
            }
        }
        false
    }

    pub(super) fn current_pos(&mut self) -> Result<usize> {
        self.close_extra(false)?;
        let mut pos = 0;
        for index in (0..=self.open).rev() {
            pos += self.nodes[index]
                .content
                .iter()
                .map(Item::node_size)
                .sum::<usize>();
            if index > 0 {
                pos += 1;
            }
        }
        Ok(pos)
    }

    fn textblock_from_context(&self) -> Result<Option<NodeType<'p>>> {
        if let Some(context) = &self.options.context {
            for depth in (0..=context.depth()).rev() {
                let node = context.node(depth);
                let found = node
                    .content_match_at(context.index_after(depth))?
                    .default_type();
                if let Some(found) = found
                    && found.is_textblock()
                    && found.default_attrs().is_some()
                {
                    return Ok(Some(self.schema().node_type_at(found.index())));
                }
            }
        }
        Ok(self
            .schema()
            .node_types()
            .find(|node_type| node_type.is_textblock() && node_type.default_attrs().is_some()))
    }
}

/// Whether a mark of this type could apply to a node of this type anywhere in the schema.
fn mark_may_apply(schema: &Schema, mark_type: &MarkType, node_type: &NodeType) -> bool {
    /// Whether an edge of this type leaves a state reachable from `start`: ProseMirror's `scan`,
    /// walking the automaton, which can be any length, without recursing.
    fn reaches(start: ContentMatch, node_type: &NodeType) -> bool {
        let mut seen = vec![start];
        let mut unvisited = vec![start];
        while let Some(matched) = unvisited.pop() {
            for index in 0..matched.edge_count() {
                let (edge_type, next) = matched.edge(index).expect("an edge in range");
                if edge_type == *node_type {
                    return true;
                }
                if !seen.contains(&next) {
                    seen.push(next);
                    unvisited.push(next);
                }
            }
        }
        false
    }
    schema.node_types().any(|parent| {
        parent.allows_mark_type(mark_type) && reaches(parent.content_match(), node_type)
    })
}
