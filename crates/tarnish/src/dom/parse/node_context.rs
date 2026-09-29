//! A node being parsed into.

use super::PreserveWhitespace;
use super::html::{BLOCK_TAGS, trailing_spaces};
use crate::Result;
use crate::dom::Dom;
use crate::json::Map;
use crate::model::{ContentMatch, Fragment, Mark, Node, NodeType, Whitespace};

/// How a context treats whitespace, and whether it is open at the start.
#[derive(Clone, Copy)]
pub(super) struct WsOptions {
    pub(super) preserve: PreserveWhitespace,
    /// Whether nothing comes before the context at the open start of an open parse, where its
    /// content needn't fit from its type's start.
    pub(super) open_left: bool,
}

impl WsOptions {
    /// The options of a node of this type opened in a context with these: the whitespace its
    /// rule sets, or all for a `pre` type, or the context's; and open at the start when first
    /// in a context that is.
    pub(super) fn child(
        self,
        node_type: &NodeType,
        preserve: Option<PreserveWhitespace>,
        first: bool,
    ) -> WsOptions {
        let preserve = match preserve {
            Some(preserve) => preserve,
            None if node_type.whitespace() == Whitespace::Pre => PreserveWhitespace::Full,
            None => self.preserve,
        };
        WsOptions {
            preserve,
            open_left: self.open_left && first,
        }
    }
}

/// A node being built while parsing, of a type of the parser's schema.
pub(super) struct NodeContext<'s> {
    /// An identity, for finding the context again after the stack has changed.
    pub(super) id: usize,
    /// The node's type, which only the top of an open parse without a top node lacks.
    pub(super) node_type: Option<NodeType<'s>>,
    pub(super) attrs: Option<Map>,
    pub(super) marks: Vec<Mark<'static>>,
    pub(super) solid: bool,
    pub(super) matched: Option<ContentMatch<'s>>,
    pub(super) ws: WsOptions,
    pub(super) content: Vec<Node<'static>>,
}

impl<'s> NodeContext<'s> {
    /// A context with no content yet. Without a match given, its content starts at its type's
    /// start, unless it is open at the start, where the match is found from the content.
    pub(super) fn new(
        id: usize,
        node_type: Option<NodeType<'s>>,
        attrs: Option<Map>,
        marks: Vec<Mark<'static>>,
        solid: bool,
        matched: Option<ContentMatch<'s>>,
        ws: WsOptions,
    ) -> NodeContext<'s> {
        let matched = match (matched, &node_type) {
            (Some(matched), _) => Some(matched),
            (None, Some(node_type)) if !ws.open_left => Some(node_type.content_match()),
            (None, _) => None,
        };
        NodeContext {
            id,
            node_type,
            attrs,
            marks,
            solid,
            matched,
            ws,
            content: Vec::new(),
        }
    }

    pub(super) fn find_wrapping(&mut self, node: &Node) -> Result<Option<Vec<NodeType<'s>>>> {
        if self.matched.is_none() {
            let Some(node_type) = &self.node_type else {
                return Ok(Some(Vec::new()));
            };
            let start = node_type.content_match();
            match start.fill_before(&Fragment::from_node(node.clone()), false, 0)? {
                Some(fill) => {
                    self.matched = start.match_fragment(&fill);
                }
                None => {
                    let wrap = start.find_wrapping(&node.node_type());
                    if wrap.is_some() {
                        self.matched = Some(start);
                    }
                    return Ok(wrap);
                }
            }
        }
        Ok(self
            .matched
            .as_ref()
            .and_then(|matched| matched.find_wrapping(&node.node_type())))
    }

    /// Whether text here is inline: by the node's type, or else by its first child, or else by
    /// whether the text's DOM parent is anything but a block.
    pub(super) fn inline_context<D: Dom>(&self, dom: &D, text: Option<&D::Node>) -> Result<bool> {
        if let Some(node_type) = &self.node_type {
            return Ok(node_type.inline_content());
        }
        if let Some(first) = self.content.first() {
            return Ok(first.is_inline());
        }
        match text.map(|text| dom.parent(text)).transpose()?.flatten() {
            Some(parent) => {
                let name = dom.node_name(&parent)?.to_lowercase();
                Ok(!BLOCK_TAGS.contains(&name.as_str()))
            }
            None => Ok(false),
        }
    }

    /// The content, without the whitespace it ends in unless it keeps whitespace, and filled out
    /// to fit unless its end is open.
    fn take_content(&mut self, open_end: bool) -> Result<Fragment<'static>> {
        if self.ws.preserve == PreserveWhitespace::No
            && let Some(last) = self.content.last_mut()
            && let Some(text) = last.text().map(|text| text.to_text())
        {
            let kept = text.len() - trailing_spaces(&text);
            if kept == 0 {
                self.content.pop();
            } else if kept < text.len() {
                *last = last.cut(0, kept)?;
            }
        }
        let mut content = Fragment::from_array(std::mem::take(&mut self.content));
        if !open_end
            && let Some(matched) = &self.matched
            && let Some(fill) = matched.fill_before(&Fragment::empty(), true, 0)?
        {
            content = content.append(&fill);
        }
        Ok(content)
    }

    /// Finish the top of an open parse, as the content parsed into it.
    pub(super) fn finish_content(mut self, open_end: bool) -> Result<Fragment<'static>> {
        self.take_content(open_end)
    }

    /// Finish the node. Only the top of an open parse can lack a type, and it is finished with
    /// [`NodeContext::finish_content`].
    pub(super) fn finish_node(mut self, open_end: bool) -> Result<Node<'static>> {
        let content = self.take_content(open_end)?;
        let node_type = self.node_type.expect("a context with a type");
        node_type.create(self.attrs.as_ref(), content, &self.marks)
    }
}
