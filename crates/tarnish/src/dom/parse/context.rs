//! The stack of nodes being parsed into, and fitting nodes into it.

use super::DomParser;
use super::node_context::{NodeContext, WsOptions};
use super::{ParseOptions, PreserveWhitespace};
use crate::dom::Dom;
use crate::error::Result;
use crate::model::{Attrs, ContentMatch, Fragment, Mark, MarkType, Node, NodeType, Schema};

pub(super) struct ParseContext<'p, 'o, D: Dom> {
    pub(super) parser: &'p DomParser<D::Node>,
    pub(super) dom: &'p D,
    pub(super) options: ParseOptions<'o, D::Node>,
    pub(super) is_open: bool,
    /// The index of the context content goes into. Those above it wait to be closed.
    pub(super) open: usize,
    pub(super) needs_block: bool,
    pub(super) nodes: Vec<NodeContext>,
    pub(super) local_preserve_ws: bool,
    next_id: usize,
}

impl<'p, 'o, D: Dom> ParseContext<'p, 'o, D> {
    pub(super) fn new(
        parser: &'p DomParser<D::Node>,
        dom: &'p D,
        options: ParseOptions<'o, D::Node>,
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
                let node_type = top_node.node_type();
                let matched = match &options.top_match {
                    Some(top_match) => top_match.clone(),
                    None => node_type.content_match(),
                };
                let attrs = Some(top_node.attrs().clone());
                NodeContext::new(
                    0,
                    Some(node_type.clone()),
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
        }
    }

    pub(super) fn top(&self) -> &NodeContext {
        &self.nodes[self.open]
    }

    pub(super) fn top_mut(&mut self) -> &mut NodeContext {
        &mut self.nodes[self.open]
    }

    pub(super) fn schema(&self) -> &'p Schema {
        &self.parser.schema
    }

    /// Find a place for a node: the context to put it in, leaving contexts that aren't solid
    /// and opening wrappers where needed. The marks the node gets, or `None` when it fits
    /// nowhere.
    pub(super) fn find_place(
        &mut self,
        node: &Node,
        marks: Vec<Mark>,
        cautious: bool,
    ) -> Result<Option<Vec<Mark>>> {
        let mut route: Option<(Vec<NodeType>, usize)> = None;
        let mut penalty = 0;
        for depth in (0..=self.open).rev() {
            let context = &mut self.nodes[depth];
            let found = context.find_wrapping(node)?;
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
        node: Node,
        marks: &[Mark],
        cautious: bool,
    ) -> Result<bool> {
        let mut marks = marks.to_vec();
        if node.is_inline()
            && self.needs_block
            && self.top().node_type.is_none()
            && let Some(block) = self.textblock_from_context()?
        {
            marks = self.enter_inner(&block, None, marks, false, None)?;
        }
        let Some(inner_marks) = self.find_place(&node, marks, cautious)? else {
            return Ok(false);
        };
        self.close_extra(false)?;
        let schema = self.schema();
        let top = self.top_mut();
        if let Some(matched) = &top.matched {
            top.matched = matched.match_type(node.node_type());
        }
        let mut node_marks = Mark::none();
        for mark in inner_marks.iter().chain(node.marks().iter()) {
            let applies = match &top.node_type {
                Some(node_type) => node_type.allows_mark_type(mark.mark_type()),
                None => mark_may_apply(schema, mark.mark_type(), node.node_type()),
            };
            if applies {
                node_marks = mark.add_to_set(&node_marks);
            }
        }
        top.content.push(node.mark(node_marks));
        Ok(true)
    }

    /// Start a node of this type, adjusting the context where it needs to. The marks left for
    /// its content, or `None` when it fits nowhere.
    pub(super) fn enter(
        &mut self,
        node_type: &NodeType,
        attrs: Option<Attrs>,
        marks: Vec<Mark>,
        preserve: Option<PreserveWhitespace>,
    ) -> Result<Option<Vec<Mark>>> {
        let created = node_type.create(attrs.as_deref(), Fragment::empty(), &[])?;
        if self.find_place(&created, marks.clone(), false)?.is_none() {
            return Ok(None);
        }
        self.enter_inner(node_type, attrs, marks, true, preserve)
            .map(Some)
    }

    /// Open a node of this type, giving it the marks it allows and leaving the rest.
    fn enter_inner(
        &mut self,
        node_type: &NodeType,
        attrs: Option<Attrs>,
        marks: Vec<Mark>,
        solid: bool,
        preserve: Option<PreserveWhitespace>,
    ) -> Result<Vec<Mark>> {
        self.close_extra(false)?;
        let schema = self.schema();
        let top = self.top_mut();
        top.matched = top
            .matched
            .as_ref()
            .and_then(|matched| matched.match_type(node_type));
        let ws = top.ws.child(node_type, preserve, top.content.is_empty());
        let mut apply = Mark::none();
        let mut rest = Vec::new();
        for mark in marks {
            let applies = match &top.node_type {
                Some(parent) => parent.allows_mark_type(mark.mark_type()),
                None => mark_may_apply(schema, mark.mark_type(), node_type),
            };
            if applies {
                apply = mark.add_to_set(&apply);
            } else {
                rest.push(mark);
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        let node_type = Some(node_type.clone());
        let context = NodeContext::new(id, node_type, attrs, apply.to_vec(), solid, None, ws);
        self.nodes.push(context);
        self.open += 1;
        Ok(rest)
    }

    /// Finish the nodes above the open one and add them to their parents.
    fn close_extra(&mut self, open_end: bool) -> Result<()> {
        while self.nodes.len() - 1 > self.open {
            let context = self.nodes.pop().expect("a node above the open one");
            let node = context.finish_node(open_end)?;
            self.nodes.last_mut().expect("a parent").content.push(node);
        }
        Ok(())
    }

    /// Close every node but the top one, and give it back.
    pub(super) fn finish(mut self) -> Result<NodeContext> {
        self.open = 0;
        self.close_extra(self.is_open)?;
        Ok(self.nodes.swap_remove(0))
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
                .map(Node::node_size)
                .sum::<usize>();
            if index > 0 {
                pos += 1;
            }
        }
        Ok(pos)
    }

    fn textblock_from_context(&self) -> Result<Option<NodeType>> {
        if let Some(context) = &self.options.context {
            for depth in (0..=context.depth()).rev() {
                let found = context
                    .node(depth)
                    .content_match_at(context.index_after(depth))?
                    .default_type();
                if let Some(found) = found
                    && found.is_textblock()
                    && found.default_attrs().is_some()
                {
                    return Ok(Some(found));
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
    fn scan(matched: &ContentMatch, node_type: &NodeType, seen: &mut Vec<ContentMatch>) -> bool {
        seen.push(matched.clone());
        for index in 0..matched.edge_count() {
            let (edge_type, next) = matched.edge(index).expect("an edge in range");
            if edge_type == *node_type {
                return true;
            }
            if !seen.contains(&next) && scan(&next, node_type, seen) {
                return true;
            }
        }
        false
    }
    schema.node_types().any(|parent| {
        parent.allows_mark_type(mark_type)
            && scan(&parent.content_match(), node_type, &mut Vec::new())
    })
}
