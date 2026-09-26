//! A node being parsed into.

use super::PreserveWhitespace;
use super::html::is_html_space;
use crate::error::Result;
use crate::model::{Attrs, ContentMatch, Fragment, Mark, Node, NodeType, Whitespace};

pub(super) const OPT_PRESERVE_WS: u8 = 1;
pub(super) const OPT_PRESERVE_WS_FULL: u8 = 2;
pub(super) const OPT_OPEN_LEFT: u8 = 4;

pub(super) fn ws_options_for(
    node_type: Option<&NodeType>,
    preserve: Option<PreserveWhitespace>,
    base: u8,
) -> u8 {
    if let Some(preserve) = preserve {
        return match preserve {
            PreserveWhitespace::No => 0,
            PreserveWhitespace::Yes => OPT_PRESERVE_WS,
            PreserveWhitespace::Full => OPT_PRESERVE_WS | OPT_PRESERVE_WS_FULL,
        };
    }
    match node_type {
        Some(node_type) if node_type.whitespace() == Whitespace::Pre => {
            OPT_PRESERVE_WS | OPT_PRESERVE_WS_FULL
        }
        _ => base & !OPT_OPEN_LEFT,
    }
}

pub(super) enum Finished {
    Node(Node),
    Fragment(Fragment),
}

/// A node being built while parsing.
pub(super) struct NodeContext {
    /// An identity, for finding the context again after the stack has changed.
    pub(super) id: usize,
    pub(super) node_type: Option<NodeType>,
    pub(super) attrs: Option<Attrs>,
    pub(super) marks: Vec<Mark>,
    pub(super) solid: bool,
    pub(super) matched: Option<ContentMatch>,
    pub(super) options: u8,
    pub(super) content: Vec<Node>,
}

impl NodeContext {
    pub(super) fn find_wrapping(&mut self, node: &Node) -> Result<Option<Vec<NodeType>>> {
        if self.matched.is_none() {
            let Some(node_type) = &self.node_type else {
                return Ok(Some(Vec::new()));
            };
            let start = node_type.content_match();
            match start.fill_before(&Fragment::from_node(node.clone()), false, 0)? {
                Some(fill) => {
                    self.matched = start.match_fragment(&fill, 0, fill.child_count());
                }
                None => {
                    let wrap = start.find_wrapping(node.node_type());
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
            .and_then(|matched| matched.find_wrapping(node.node_type())))
    }

    pub(super) fn finish(mut self, open_end: bool) -> Result<Finished> {
        if self.options & OPT_PRESERVE_WS == 0
            && let Some(last) = self.content.last()
            && let Some(text) = last.text()
        {
            let units = text.units();
            let kept = units.len()
                - units
                    .iter()
                    .rev()
                    .take_while(|&&unit| is_html_space(unit))
                    .count();
            if kept < units.len() {
                if kept == 0 {
                    self.content.pop();
                } else {
                    let cut = last.cut(0, kept)?;
                    *self.content.last_mut().expect("a last node") = cut;
                }
            }
        }
        let mut content = Fragment::from_array(self.content);
        if !open_end
            && let Some(matched) = &self.matched
            && let Some(fill) = matched.fill_before(&Fragment::empty(), true, 0)?
        {
            content = content.append(&fill);
        }
        Ok(match &self.node_type {
            Some(node_type) => {
                Finished::Node(node_type.create(self.attrs.as_deref(), content, &self.marks)?)
            }
            None => Finished::Fragment(content),
        })
    }
}
