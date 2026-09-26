//! Walking the DOM, and parsing each node by the rules.

use super::context::ParseContext;
use super::html::{
    BLOCK_TAGS, IGNORE_TAGS, collapse_spaces, is_html_space, is_list_tag, normalize_list,
    normalize_newlines, split_lines,
};
use super::node_context::{OPT_PRESERVE_WS, OPT_PRESERVE_WS_FULL};
use super::rule::{ContentElement, ParseRule, PreserveWhitespace, Skip};
use super::{Matched, RuleRef};
use crate::dom::{Dom, NodeKind};
use crate::error::{Error, Result};
use crate::model::{Fragment, Mark, MarkType, Node, NodeType};
use crate::stack;
use crate::text::{Text, is_js_space};

/// A text to add: a DOM text node's, or the newline a `<br>` stands for.
pub(super) struct TextSource<'n, N> {
    dom: Option<&'n N>,
    value: Text,
}

impl<'p, 'o, D: Dom> ParseContext<'p, 'o, D> {
    pub(super) fn add_dom(&mut self, node: &D::Node, marks: &[Mark]) -> Result<()> {
        match self.dom.kind(node)? {
            NodeKind::Text => {
                let value = self.dom.text(node)?;
                self.add_text_node(
                    TextSource {
                        dom: Some(node),
                        value,
                    },
                    marks,
                )
            }
            NodeKind::Element => self.add_element(node, marks, None),
            NodeKind::Other => Ok(()),
        }
    }

    pub(super) fn inline_context(&self, source: &TextSource<'_, D::Node>) -> Result<bool> {
        let top = self.top();
        if let Some(node_type) = &top.node_type {
            return Ok(node_type.inline_content());
        }
        if let Some(first) = top.content.first() {
            return Ok(first.is_inline());
        }
        let Some(dom) = source.dom else {
            return Ok(false);
        };
        match self.dom.parent(dom)? {
            Some(parent) => {
                let name = self.dom.node_name(&parent)?.to_lowercase();
                Ok(!BLOCK_TAGS.contains(&name.as_str()))
            }
            None => Ok(false),
        }
    }

    pub(super) fn add_text_node(
        &mut self,
        source: TextSource<'_, D::Node>,
        marks: &[Mark],
    ) -> Result<()> {
        let top_options = self.top().options;
        let preserve = if top_options & OPT_PRESERVE_WS_FULL != 0 {
            PreserveWhitespace::Full
        } else if self.local_preserve_ws || top_options & OPT_PRESERVE_WS != 0 {
            PreserveWhitespace::Yes
        } else {
            PreserveWhitespace::No
        };
        let mut value: Vec<u16> = source.value.units().to_vec();
        let schema = self.schema();
        if preserve == PreserveWhitespace::Full
            || self.inline_context(&source)?
            || value.iter().any(|&unit| !is_html_space(unit))
        {
            if preserve == PreserveWhitespace::No {
                value = collapse_spaces(&value);
                // Leading space goes when nothing comes before it, or a hard break, or text
                // that ends in space.
                if value.first().is_some_and(|&unit| is_html_space(unit))
                    && self.open == self.nodes.len() - 1
                {
                    let node_before = self.top().content.last();
                    let dom_before = match source.dom {
                        Some(dom) => self.dom.previous_sibling(dom)?,
                        None => None,
                    };
                    let after_break = match &dom_before {
                        Some(before) => self.dom.node_name(before)? == "BR",
                        None => false,
                    };
                    let after_space = node_before.is_some_and(|before| {
                        before
                            .text()
                            .and_then(|text| text.units().last().copied())
                            .is_some_and(is_html_space)
                    });
                    if node_before.is_none() || after_break || after_space {
                        value.remove(0);
                    }
                }
            } else if preserve == PreserveWhitespace::Full {
                value = normalize_newlines(&value, &[0x0a]);
            } else if let Some(linebreak) = schema.linebreak_replacement()
                && value.iter().any(|&unit| unit == 0x0a || unit == 0x0d)
                && self
                    .top_mut()
                    .find_wrapping(&linebreak.create(None, Fragment::empty(), &[])?)?
                    .is_some()
            {
                for (index, line) in split_lines(&value).into_iter().enumerate() {
                    if index > 0 {
                        self.insert_node(
                            linebreak.create(None, Fragment::empty(), &[])?,
                            marks,
                            true,
                        )?;
                    }
                    if !line.is_empty() {
                        let blank = !line.iter().any(|&unit| !is_js_space(unit));
                        self.insert_node(schema.text(line, &[])?, marks, blank)?;
                    }
                }
                value.clear();
            } else {
                value = normalize_newlines(&value, &[0x20]);
            }
            if !value.is_empty() {
                let blank = !value.iter().any(|&unit| !is_js_space(unit));
                self.insert_node(schema.text(value, &[])?, marks, blank)?;
            }
            if let Some(dom) = source.dom {
                self.find_in_text(dom, &source.value)?;
            }
        } else if let Some(dom) = source.dom {
            self.find_inside(dom)?;
        }
        Ok(())
    }

    /// Parse an element by the first rule that matches it, or its content when none does.
    pub(super) fn add_element(
        &mut self,
        node: &D::Node,
        marks: &[Mark],
        match_after: Option<usize>,
    ) -> Result<()> {
        let dom = self.dom;
        let outer_ws = self.local_preserve_ws;
        let mut top = self.top().id;
        let name = dom.node_name(node)?;
        if name == "PRE" || dom.style_value(node, "white-space")?.contains("pre") {
            self.local_preserve_ws = true;
        }
        let name = name.to_lowercase();
        if is_list_tag(&name) && self.parser.normalize_lists {
            normalize_list(dom, node)?;
        }
        let from_node = match self.options.rule_from_node {
            Some(rule_from_node) => rule_from_node(node)?,
            None => None,
        };
        let matched = match from_node {
            Some(rule) => Some(Matched {
                attrs: rule.attrs.clone(),
                rule: RuleRef::FromNode(Box::new(rule)),
            }),
            None => self.parser.match_tag(dom, node, self, match_after)?,
        };
        let rule = matched.as_ref().map(|matched| match &matched.rule {
            RuleRef::Listed(index) => &self.parser.tags[*index],
            RuleRef::FromNode(rule) => &**rule,
        });
        let ignore = match rule {
            Some(rule) => rule.ignore,
            None => IGNORE_TAGS.contains(&name.as_str()),
        };
        if ignore {
            self.find_inside(node)?;
            self.ignore_fallback(node, marks)?;
        } else if rule.is_none_or(|rule| !matches!(rule.skip, Skip::No) || rule.close_parent) {
            let mut node = node.clone();
            if let Some(rule) = rule {
                if rule.close_parent {
                    self.open = self.open.saturating_sub(1);
                } else if let Skip::Node(skip) = &rule.skip {
                    node = skip.clone();
                }
            }
            let skip = rule.is_some_and(|rule| !matches!(rule.skip, Skip::No));
            let mut sync = false;
            let old_needs_block = self.needs_block;
            if BLOCK_TAGS.contains(&name.as_str()) {
                // `top` is the context that was open when the element came, which closing its
                // parent leaves on the stack.
                if self
                    .context(top)
                    .content
                    .first()
                    .is_some_and(Node::is_inline)
                    && self.open > 0
                {
                    self.open -= 1;
                    top = self.top().id;
                }
                sync = true;
                if self.context(top).node_type.is_none() {
                    self.needs_block = true;
                }
            } else if dom.first_child(&node)?.is_none() {
                self.leaf_fallback(&node, marks)?;
                self.local_preserve_ws = outer_ws;
                return Ok(());
            }
            let inner_marks = if skip {
                Some(marks.to_vec())
            } else {
                self.read_styles(&node, marks)?
            };
            if let Some(inner_marks) = inner_marks {
                self.add_all(&node, &inner_marks, None, None)?;
            }
            if sync {
                self.sync(top);
            }
            self.needs_block = old_needs_block;
        } else if let Some(inner_marks) = self.read_styles(node, marks)? {
            let matched = matched.expect("a matched rule");
            let continue_after = match &matched.rule {
                RuleRef::Listed(index) if !self.parser.tags[*index].consuming => Some(*index),
                _ => None,
            };
            self.add_element_by_rule(node, &matched, &inner_marks, continue_after)?;
        }
        self.local_preserve_ws = outer_ws;
        Ok(())
    }

    /// Called for a leaf DOM node that would otherwise be ignored.
    pub(super) fn leaf_fallback(&mut self, node: &D::Node, marks: &[Mark]) -> Result<()> {
        if self.dom.node_name(node)? == "BR"
            && self
                .top()
                .node_type
                .as_ref()
                .is_some_and(NodeType::inline_content)
        {
            let source = TextSource {
                dom: None,
                value: Text::from("\n"),
            };
            self.add_text_node(source, marks)?;
        }
        Ok(())
    }

    /// Called for an ignored node.
    pub(super) fn ignore_fallback(&mut self, node: &D::Node, marks: &[Mark]) -> Result<()> {
        // An ignored <br> still makes an inline context.
        if self.dom.node_name(node)? == "BR"
            && !self
                .top()
                .node_type
                .as_ref()
                .is_some_and(NodeType::inline_content)
        {
            let dash = self.schema().text("-", &[])?;
            self.find_place(&dash, marks.to_vec(), true)?;
        }
        Ok(())
    }

    /// The marks with those the element's styles add or clear, `None` when a style's rule
    /// ignores the element.
    pub(super) fn read_styles(
        &mut self,
        node: &D::Node,
        marks: &[Mark],
    ) -> Result<Option<Vec<Mark>>> {
        let mut marks = marks.to_vec();
        if self.dom.style_count(node)? == 0 {
            return Ok(Some(marks));
        }
        let parser = self.parser;
        for property in &parser.matched_styles {
            let value = self.dom.style_value(node, property)?;
            if value.is_empty() {
                continue;
            }
            let mut after = None;
            while let Some((index, attrs)) = parser.match_style(property, &value, self, after)? {
                let rule = &parser.styles[index];
                if rule.ignore {
                    return Ok(None);
                }
                if let Some(clear_mark) = &rule.clear_mark {
                    let mut kept = Vec::with_capacity(marks.len());
                    for mark in marks {
                        if !clear_mark(&mark)? {
                            kept.push(mark);
                        }
                    }
                    marks = kept;
                } else {
                    marks.push(self.rule_mark_type(rule)?.create(attrs.as_deref())?);
                }
                if rule.consuming {
                    break;
                }
                after = Some(index);
            }
        }
        Ok(Some(marks))
    }

    pub(super) fn rule_mark_type(&self, rule: &ParseRule<D::Node>) -> Result<MarkType> {
        let name = rule.mark.as_deref().unwrap_or("undefined");
        self.schema()
            .mark_type(name)
            .ok_or_else(|| Error::Other(format!("No mark type {name} in the schema")))
    }

    pub(super) fn add_element_by_rule(
        &mut self,
        node: &D::Node,
        matched: &Matched<D::Node>,
        marks: &[Mark],
        continue_after: Option<usize>,
    ) -> Result<()> {
        let rule = match &matched.rule {
            RuleRef::Listed(index) => &self.parser.tags[*index],
            RuleRef::FromNode(rule) => &**rule,
        };
        let mut marks = marks.to_vec();
        let mut sync = false;
        let mut leaf = false;
        if let Some(name) = &rule.node {
            let node_type = self.schema().expect_node_type(name)?;
            if !node_type.is_leaf() {
                let preserve = rule.tag().and_then(|tag| tag.preserve_whitespace);
                if let Some(inner) =
                    self.enter(&node_type, matched.attrs.clone(), marks.clone(), preserve)?
                {
                    sync = true;
                    marks = inner;
                }
            } else {
                leaf = true;
                let is_break = self.dom.node_name(node)? == "BR";
                let created = node_type.create(matched.attrs.as_deref(), Fragment::empty(), &[])?;
                if !self.insert_node(created, &marks, is_break)? {
                    self.leaf_fallback(node, &marks)?;
                }
            }
        } else {
            marks.push(
                self.rule_mark_type(rule)?
                    .create(matched.attrs.as_deref())?,
            );
        }
        let start_in = self.top().id;
        let tag = rule.tag();
        if leaf {
            self.find_inside(node)?;
        } else if let Some(after) = continue_after {
            self.add_element(node, &marks, Some(after))?;
        } else if let Some(get_content) = tag.and_then(|tag| tag.get_content.as_ref()) {
            self.find_inside(node)?;
            for child in get_content(node, self.schema())?.children() {
                self.insert_node(child.clone(), &marks, false)?;
            }
        } else {
            let content_dom = match tag.and_then(|tag| tag.content_element.as_ref()) {
                None => node.clone(),
                Some(ContentElement::Selector(selector)) => self
                    .dom
                    .query_selector(node, selector)?
                    .ok_or_else(|| Error::Other(format!("No element matches {selector}")))?,
                Some(ContentElement::Hook(hook)) => hook(node)?,
                Some(ContentElement::Node(content)) => content.clone(),
            };
            self.find_around(node, &content_dom, true)?;
            self.add_all(&content_dom, &marks, None, None)?;
            self.find_around(node, &content_dom, false)?;
        }
        if sync && self.sync(start_in) {
            self.open -= 1;
        }
        Ok(())
    }

    /// Add the parent's children from index `start` to `end`, or all of them.
    pub(super) fn add_all(
        &mut self,
        parent: &D::Node,
        marks: &[Mark],
        start: Option<usize>,
        end: Option<usize>,
    ) -> Result<()> {
        let dom = self.dom;
        let mut index = start.unwrap_or(0);
        let mut child = match start {
            Some(start) if start > 0 => dom.child(parent, start)?,
            _ => dom.first_child(parent)?,
        };
        let end = match end {
            Some(end) => dom.child(parent, end)?,
            None => None,
        };
        loop {
            let at_end = match (&child, &end) {
                (Some(child), Some(end)) => dom.same(child, end)?,
                (None, None) => true,
                (None, Some(_)) => true,
                (Some(_), None) => false,
            };
            if at_end {
                break;
            }
            let current = child.expect("a child before the end");
            self.find_at_point(parent, index)?;
            stack::grow(|| self.add_dom(&current, marks))?;
            child = dom.next_sibling(&current)?;
            index += 1;
        }
        self.find_at_point(parent, index)
    }
}
