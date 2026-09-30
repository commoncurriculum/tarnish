//! Walking the DOM, and parsing each node by the rules.

use std::borrow::Cow;

use super::Matched;
use super::context::{Inserted, ParseContext};
use super::html::{
    BLOCK_TAGS, IGNORE_TAGS, collapse_spaces, has_non_space, is_html_space, is_list_tag,
    normalize_list,
};
use super::node_context::Item;
use super::rule::{Content, ContentElement, ElementRule, PreserveWhitespace, Skip};
use crate::dom::{Dom, NodeKind};
use crate::js::stack;
use crate::js::text::{Text, is_blank, line_breaks, replace_line_breaks, split_lines};
use crate::js::{self, value::Nullish};
use crate::model::{Mark, MarkType, NodeType};
use crate::{Error, Result};

/// A text to add: a DOM text node's, or the newline a `<br>` stands for.
struct TextSource<'n, N> {
    dom: Option<&'n N>,
    value: Text,
}

impl<'p, D: Dom> ParseContext<'p, D> {
    fn add_dom(&mut self, node: &D::Node, marks: &[Mark<'static>]) -> Result<()> {
        self.deadline.turn(marks.len() + self.open)?;
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

    fn add_text_node(
        &mut self,
        source: TextSource<'_, D::Node>,
        marks: &[Mark<'static>],
    ) -> Result<()> {
        let local = match self.local_preserve_ws {
            true => PreserveWhitespace::Yes,
            false => PreserveWhitespace::No,
        };
        let preserve = self.top().ws.preserve.max(local);
        let schema = self.schema();
        let text = &source.value;
        if !(preserve == PreserveWhitespace::Full
            || self.top().inline_context(self.dom, source.dom)?
            || has_non_space(text))
        {
            // JavaScript looks for positions inside the dropped text, which holds none, as only
            // an element can.
            return Ok(());
        }
        let value = match preserve {
            PreserveWhitespace::No => {
                let value = collapse_spaces(text);
                if value.first_unit().is_some_and(is_html_space)
                    && self.open == self.nodes.len() - 1
                    && self.strips_leading_space(source.dom)?
                {
                    value.slice(1, value.len())
                } else {
                    value
                }
            }
            PreserveWhitespace::Full => replace_line_breaks(text, '\n'),
            PreserveWhitespace::Yes => match schema.linebreak_replacement() {
                Some(linebreak)
                    if line_breaks(text).next().is_some()
                        && linebreak.check_create(None).map(|()| true)?
                        && self.top_mut().find_wrapping(&linebreak)?.is_some() =>
                {
                    for (index, line) in split_lines(text).enumerate() {
                        if index > 0 {
                            self.insert_node(Inserted::Leaf(linebreak.clone(), None), marks, true)?;
                        }
                        if !line.is_empty() {
                            let blank = is_blank(&line);
                            self.insert_node(Inserted::Text(line), marks, blank)?;
                        }
                    }
                    Text::default()
                }
                _ => replace_line_breaks(text, ' '),
            },
        };
        if !value.is_empty() {
            let blank = is_blank(&value);
            self.insert_node(Inserted::Text(value), marks, blank)?;
        }
        if let Some(dom) = source.dom {
            self.find_in_text(dom, text)?;
        }
        Ok(())
    }

    /// Whether a text's leading space goes: when nothing comes before it, or a hard break, or
    /// text that ends in space.
    fn strips_leading_space(&self, text: Option<&D::Node>) -> Result<bool> {
        let Some(before) = self.top().content.last() else {
            return Ok(true);
        };
        let sibling = match text {
            Some(text) => self.dom.previous_sibling(text)?,
            None => None,
        };
        if let Some(sibling) = sibling
            && self.dom.node_name(&sibling)? == "BR"
        {
            return Ok(true);
        }
        let last = before.text().and_then(|text| text.last_unit());
        Ok(last.is_some_and(is_html_space))
    }

    /// Parse an element by the first rule that matches it, or its content when none does.
    fn add_element(
        &mut self,
        node: &D::Node,
        marks: &[Mark<'static>],
        match_after: Option<usize>,
    ) -> Result<()> {
        let (dom, parser) = (self.dom, self.parser);
        let outer_ws = self.local_preserve_ws;
        let name = dom.node_name(node)?;
        if name == "PRE" || dom.style_value(node, "white-space")?.contains("pre") {
            self.local_preserve_ws = true;
        }
        if parser.normalize_lists && is_list_tag(&name.to_lowercase()) {
            normalize_list(dom, node)?;
        }
        let from_node = match self.options.rule_from_node {
            Some(rule_from_node) => rule_from_node(node)?,
            None => None,
        };
        let matched = match &from_node {
            Some(rule) => Some(Matched::from_node(rule)),
            None => parser.match_tag(dom, node, self, match_after)?,
        };
        let ignore = match &matched {
            Some(matched) => matched.ignore,
            None => IGNORE_TAGS.contains(&name.to_lowercase().as_str()),
        };
        match matched {
            _ if ignore => {
                self.find_inside(node)?;
                self.ignore_fallback(&name, marks)?;
            }
            Some(matched)
                if matches!(matched.element.skip, Skip::No) && !matched.element.close_parent =>
            {
                if let Some(inner_marks) = self.read_styles(node, marks)? {
                    self.add_element_by_rule(node, &name, &matched, &inner_marks)?;
                }
            }
            matched => {
                let rule = matched.map(|matched| matched.element);
                self.add_element_content(node, &name, marks, rule)?;
            }
        }
        self.local_preserve_ws = outer_ws;
        Ok(())
    }

    /// Parse the content of an element that makes nothing itself: no rule matched it, or its
    /// rule skips it or closes the parent.
    fn add_element_content(
        &mut self,
        node: &D::Node,
        name: &str,
        marks: &[Mark<'static>],
        rule: Option<&ElementRule<D::Node>>,
    ) -> Result<()> {
        // The context open when the element came, which closing its parent leaves on the stack.
        let mut top = self.open;
        let skip = rule.is_some_and(|rule| !matches!(rule.skip, Skip::No));
        let replaced = match rule {
            Some(rule) if rule.close_parent => {
                self.open = self.open.saturating_sub(1);
                None
            }
            Some(ElementRule {
                skip: Skip::Node(skip),
                ..
            }) => Some(skip),
            _ => None,
        };
        let content = replaced.unwrap_or(node);
        let mut sync = None;
        let old_needs_block = self.needs_block;
        if BLOCK_TAGS.contains(&name.to_lowercase().as_str()) {
            if self.nodes[top].content.first().is_some_and(Item::is_inline) && self.open > 0 {
                self.open -= 1;
                top = self.open;
            }
            sync = Some(self.nodes[top].id);
            if self.nodes[top].node_type.is_none() {
                self.needs_block = true;
            }
        } else if self.dom.first_child(content)?.is_none() {
            let name = match replaced {
                Some(replaced) => Cow::Owned(self.dom.node_name(replaced)?),
                None => Cow::Borrowed(name),
            };
            return self.leaf_fallback(&name, marks);
        }
        let inner_marks = if skip {
            Some(marks.to_vec())
        } else {
            self.read_styles(content, marks)?
        };
        if let Some(inner_marks) = inner_marks {
            self.add_all(content, &inner_marks, None, None)?;
        }
        if let Some(sync) = sync {
            self.sync(sync);
        }
        self.needs_block = old_needs_block;
        Ok(())
    }

    /// Called for a leaf DOM node, by its name, that would otherwise be ignored.
    fn leaf_fallback(&mut self, name: &str, marks: &[Mark<'static>]) -> Result<()> {
        if name == "BR"
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

    /// Called for an ignored node, by its name.
    fn ignore_fallback(&mut self, name: &str, marks: &[Mark<'static>]) -> Result<()> {
        // An ignored <br> still makes an inline context.
        if name == "BR"
            && !self
                .top()
                .node_type
                .as_ref()
                .is_some_and(NodeType::inline_content)
        {
            // A place for the text "-".
            let text = self.schema().text_type();
            self.find_place(&text, marks.to_vec(), true)?;
        }
        Ok(())
    }

    /// The marks with those the element's styles add or clear, `None` when a style's rule
    /// ignores the element.
    fn read_styles(
        &mut self,
        node: &D::Node,
        marks: &[Mark<'static>],
    ) -> Result<Option<Vec<Mark<'static>>>> {
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
                match &rule.kind.clear_mark {
                    Some(clear_mark) => {
                        let mut kept = Vec::with_capacity(marks.len());
                        for mark in marks {
                            if !clear_mark(&mark)? {
                                kept.push(mark);
                            }
                        }
                        marks = kept;
                    }
                    None => {
                        let mark_type = self.rule_mark_type(rule.mark.as_deref())?;
                        marks.push(mark_type.create(attrs.as_ref())?);
                    }
                }
                if rule.consuming {
                    break;
                }
                after = Some(index);
            }
        }
        Ok(Some(marks))
    }

    fn rule_mark_type(&self, mark: Option<&str>) -> Result<MarkType<'p>> {
        let mark_type = mark.and_then(|name| self.schema().mark_type(name));
        mark_type.ok_or_else(|| {
            Error::Other(match mark {
                Some(name) => format!("No mark type {name} in the schema"),
                None => "A parse rule that makes neither a node nor a mark".into(),
            })
        })
    }

    /// Parse an element, by its name, as its rule says.
    fn add_element_by_rule(
        &mut self,
        node: &D::Node,
        name: &str,
        matched: &Matched<'_, D::Node>,
        marks: &[Mark<'static>],
    ) -> Result<()> {
        let rule = matched.element;
        let mut marks = marks.to_vec();
        let mut sync = false;
        let node_type = match &rule.node {
            Some(node_type) => Some(self.schema().expect_node_type(node_type)?),
            None => None,
        };
        match &node_type {
            Some(node_type) if node_type.is_leaf() => {
                node_type.check_create(matched.attrs.as_ref())?;
                let leaf = Inserted::Leaf(node_type.clone(), matched.attrs.clone());
                if !self.insert_node(leaf, &marks, name == "BR")? {
                    self.leaf_fallback(name, &marks)?;
                }
            }
            Some(node_type) => {
                let preserve = rule.preserve_whitespace;
                if let Some(inner) =
                    self.enter(node_type, matched.attrs.clone(), marks.clone(), preserve)?
                {
                    sync = true;
                    marks = inner;
                }
            }
            None => {
                let mark_type = self.rule_mark_type(matched.mark)?;
                marks.push(mark_type.create(matched.attrs.as_ref())?);
            }
        }
        let start_in = self.top().id;
        if node_type.as_ref().is_some_and(NodeType::is_leaf) {
            self.find_inside(node)?;
        } else if let Some(after) = matched.continue_after {
            self.add_element(node, &marks, Some(after))?;
        } else {
            let content_dom = match &rule.content {
                Content::Get(get_content) => {
                    self.find_inside(node)?;
                    let content = get_content(node, self.schema())?;
                    for child in content.children() {
                        self.insert_node(Inserted::Node(child), &marks, false)?;
                    }
                    None
                }
                Content::Children => Some(node.clone()),
                Content::Element(ContentElement::Selector(selector)) => {
                    let found = self.dom.query_selector(node, selector)?;
                    let found = found
                        .ok_or_else(|| Error::Other(format!("No element matches {selector}")))?;
                    Some(found)
                }
                Content::Element(ContentElement::Hook(hook)) => Some(hook(node)?),
                Content::Element(ContentElement::Node(content)) => Some(content.clone()),
            };
            if let Some(content_dom) = content_dom {
                self.find_around(node, &content_dom, true)?;
                self.add_all(&content_dom, &marks, None, None)?;
                self.find_around(node, &content_dom, false)?;
            }
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
        marks: &[Mark<'static>],
        start: Option<usize>,
        end: Option<usize>,
    ) -> Result<()> {
        let dom = self.dom;
        let mut index = start.unwrap_or(0);
        let mut child = match start {
            Some(start) if start > 0 => dom.child(parent, start)?,
            _ => dom.first_child(parent)?,
        };
        let end_child = match end {
            Some(end) => dom.child(parent, end)?,
            None => None,
        };
        loop {
            let Some(current) = child else {
                // With an end it hasn't met, ProseMirror parses on past the last child: the
                // `undefined` of an index past it, or the `null` after it.
                if end_child.is_some() {
                    let missing = match start {
                        Some(start) if start > 0 && index == start => Nullish::Undefined,
                        _ => Nullish::Null,
                    };
                    return Err(js::value::cannot_read(missing, "nodeType"));
                }
                break;
            };
            if let Some(end_child) = &end_child
                && dom.same(&current, end_child)?
            {
                break;
            }
            self.find_at_point(parent, index)?;
            stack::grow(|| self.add_dom(&current, marks))?;
            child = dom.next_sibling(&current)?;
            index += 1;
        }
        self.find_at_point(parent, index)
    }
}
