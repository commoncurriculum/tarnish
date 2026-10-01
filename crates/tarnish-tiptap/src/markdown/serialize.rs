//! `MarkdownManager.serialize` and the helpers it gives each extension's `renderMarkdown`. It
//! reads the document as the JS does, throwing where the JS would, so a malformed document
//! fails with the JS's error.

use std::sync::LazyLock;

use indexmap::IndexMap;
use rustc_hash::{FxBuildHasher, FxHashSet};
use tarnish_js::stack;

use super::MarkdownManager;
use super::utils::{
    Marks, close_marks_before_node, find_marks_to_close, find_marks_to_close_at_end,
    find_marks_to_open, mark_type, marks_by_type, reopen_marks_after_node,
};
use crate::attrs_equal;
use crate::markdown::{RenderContext, RenderHelpers};
use tarnish::json::{Map, Value, json};
use tarnish_js::value::{self, Nullish, SameValueKey};
use tarnish_js::{self as js, Error};

const PLACEHOLDER: &str = "\u{E000}__TIPTAP_MARKDOWN_PLACEHOLDER__\u{E001}";

#[derive(Clone, Copy, Default, PartialEq)]
enum OpeningMode {
    #[default]
    Markdown,
    Html,
}

/// Which side of a mark's content its Markdown is for.
#[derive(Clone, Copy)]
enum Side {
    Opening,
    Closing,
}

/// The `helpers` `renderNodeToMarkdown` gives a node's renderer.
struct NodeHelpers<'a> {
    manager: &'a MarkdownManager,
    node: &'a Value,
    index: usize,
}

impl RenderHelpers for NodeHelpers<'_> {
    fn render_children(&self, nodes: &Value, separator: &str) -> Result<String, Error> {
        let nodes = match nodes {
            Value::Array(_) => nodes,
            _ => value::get(Some(nodes), "content")?
                .filter(|content| js::truthy(Some(content)))
                .unwrap_or(nodes),
        };
        self.manager
            .render_nodes(nodes, self.node, separator, self.index)
    }

    fn render_array(&self, nodes: &[Value], separator: &str) -> Result<String, Error> {
        self.manager
            .render_nodes_with_mark_boundaries(nodes, self.node, separator)
    }
}

/// The `helpers` a mark renders with to find its opening and closing: every child is the
/// placeholder.
struct MarkHelpers;

impl RenderHelpers for MarkHelpers {
    fn render_children(&self, _: &Value, _: &str) -> Result<String, Error> {
        Ok(PLACEHOLDER.to_string())
    }

    fn render_array(&self, _: &[Value], _: &str) -> Result<String, Error> {
        Ok(PLACEHOLDER.to_string())
    }
}

impl MarkdownManager {
    /// `markdownManager.serialize(doc)`.
    pub fn serialize(&self, doc: &Value) -> Result<String, Error> {
        if !js::truthy(Some(doc)) {
            return Ok(String::new());
        }
        let result = self.render_nodes(doc, doc, "", 0)?;
        Ok(if is_empty_output(&result) {
            String::new()
        } else {
            result
        })
    }

    /// `renderNodes(nodeOrNodes, parentNode, separator, index)`.
    fn render_nodes(
        &self,
        nodes: &Value,
        parent: &Value,
        separator: &str,
        index: usize,
    ) -> Result<String, Error> {
        match nodes {
            Value::Array(nodes) => self.render_nodes_with_mark_boundaries(nodes, parent, separator),
            _ if js::truthy(value::get(Some(nodes), "type")?) => {
                self.render_node(nodes, parent, index)
            }
            _ => Ok(String::new()),
        }
    }

    /// `renderNodeToMarkdown`, for a node whose `type` the caller has read.
    fn render_node(&self, node: &Value, parent: &Value, index: usize) -> Result<String, Error> {
        let kind = value::optional(Some(node), "type");
        if kind.is_some_and(|kind| kind == "text") {
            return self.encode_text_for_markdown(node, parent);
        }
        let Some(render) = kind
            .and_then(Value::as_str)
            .and_then(|kind| self.handler_for_token(kind))
            .and_then(|spec| spec.render.as_ref())
        else {
            return Ok(String::new());
        };
        let previous_node = match value::optional(Some(parent), "content") {
            Some(Value::Array(siblings)) if index > 0 => siblings.get(index - 1),
            _ => None,
        };
        let helpers = NodeHelpers {
            manager: self,
            node,
            index,
        };
        stack::grow(|| render(node, &helpers, &RenderContext { previous_node }))
    }

    /// `renderNodesWithMarkBoundaries`.
    fn render_nodes_with_mark_boundaries(
        &self,
        nodes: &[Value],
        parent: &Value,
        separator: &str,
    ) -> Result<String, Error> {
        let mut result = Vec::with_capacity(nodes.len());
        let mut active_marks = Marks::default();
        let mut reopen_with_html_on_next_open: FxHashSet<SameValueKey> = FxHashSet::default();
        let mut opening_modes: IndexMap<SameValueKey, OpeningMode, FxBuildHasher> =
            IndexMap::default();

        for (i, node) in nodes.iter().enumerate() {
            let next_node = nodes.get(i + 1);
            let kind = value::get(Some(node), "type")?;
            if !js::truthy(kind) {
                continue;
            }

            if kind.is_none_or(|kind| kind != "text") {
                let node_marks = value::array_method(
                    value::or_empty_array(value::optional(Some(node), "marks")),
                    "(node.marks || []).map",
                )?;
                let node_mark_types = node_marks
                    .iter()
                    .map(mark_type)
                    .collect::<Result<FxHashSet<_>, _>>()?;
                let mut marks_to_reopen = Marks::default();
                let mut modes_to_reopen: IndexMap<SameValueKey, OpeningMode, FxBuildHasher> =
                    IndexMap::default();
                for (kind, mark) in &active_marks {
                    if node_mark_types.contains(kind) {
                        marks_to_reopen.insert(*kind, mark);
                        let mode = opening_modes.get(kind).copied().unwrap_or_default();
                        modes_to_reopen.insert(*kind, mode);
                    }
                }
                let before = close_marks_before_node(&mut active_marks, |kind, mark| {
                    let mode = opening_modes.get(&kind).copied().unwrap_or_default();
                    self.mark_markdown(Side::Closing, kind, Some(mark), mode)
                })?;
                opening_modes.clear();
                let content = self.render_node(node, parent, i)?;
                let after = if kind.is_some_and(|kind| kind == "hardBreak") {
                    String::new()
                } else {
                    reopen_marks_after_node(marks_to_reopen, &mut active_marks, |kind, mark| {
                        let mode = modes_to_reopen.get(&kind).copied().unwrap_or_default();
                        opening_modes.insert(kind, mode);
                        self.mark_markdown(Side::Opening, kind, Some(mark), mode)
                    })?
                };
                result.push(if before.is_empty() && after.is_empty() {
                    content
                } else {
                    [before.as_str(), &content, &after].concat()
                });
                continue;
            }

            let mut text = self.encode_text_for_markdown(node, parent)?;
            let current_marks = marks_by_type(value::array_method(
                value::or_empty_array(value::optional(Some(node), "marks")),
                "(node.marks || []).map",
            )?)?;
            let marks_to_open = self.marks_to_open(&active_marks, &current_marks, next_node)?;
            let marks_to_close = find_marks_to_close(&current_marks, next_node)?;
            let active_closing_here: Vec<SameValueKey> = marks_to_close
                .iter()
                .filter(|kind| active_marks.contains_key(*kind))
                .copied()
                .collect();
            let crossed_boundary = !active_closing_here.is_empty() && !marks_to_open.is_empty();

            let mut middle_trailing_whitespace = String::new();
            if !marks_to_close.is_empty() && !crossed_boundary {
                middle_trailing_whitespace = split_trailing_whitespace(&mut text);
            }
            if !crossed_boundary {
                for kind in marks_to_close.iter().rev() {
                    if !active_marks.contains_key(kind) {
                        continue;
                    }
                    let mode = opening_modes.get(kind).copied().unwrap_or_default();
                    let mark = current_marks.get(kind).copied();
                    text += &self.mark_markdown(Side::Closing, *kind, mark, mode)?;
                    active_marks.shift_remove(kind);
                    opening_modes.shift_remove(kind);
                }
            }

            let mut leading_whitespace = String::new();
            if !marks_to_open.is_empty() {
                let trimmed = js::trim_start(&text);
                leading_whitespace = text[..text.len() - trimmed.len()].to_string();
                text = trimmed.to_string();
            }
            for (kind, mark) in &marks_to_open {
                let mode = if reopen_with_html_on_next_open.contains(kind) {
                    OpeningMode::Html
                } else {
                    OpeningMode::Markdown
                };
                text = self.mark_markdown(Side::Opening, *kind, Some(mark), mode)? + &text;
                opening_modes.insert(*kind, mode);
                reopen_with_html_on_next_open.remove(kind);
            }
            if !crossed_boundary {
                for (kind, mark) in marks_to_open.iter().rev() {
                    active_marks.insert(*kind, mark);
                }
            }
            text = leading_whitespace + &text;

            let marks_to_close_at_end: Vec<SameValueKey> = if crossed_boundary {
                let next_marks = value::array_method(
                    value::or_empty_array(
                        next_node.and_then(|next| value::optional(Some(next), "marks")),
                    ),
                    "(nextNode?.marks || []).map",
                )?;
                let next_mark_types = next_marks
                    .iter()
                    .map(mark_type)
                    .collect::<Result<FxHashSet<_>, _>>()?;
                for (kind, _) in &marks_to_open {
                    if next_mark_types.contains(kind) && self.html_reopen(*kind).is_some() {
                        reopen_with_html_on_next_open.insert(*kind);
                    }
                }
                let position = |kind: SameValueKey| {
                    active_marks
                        .get_index_of(&kind)
                        .map_or(-1.0, |index| index as f64)
                };
                let mut lifo = active_closing_here.clone();
                js::array::sort(&mut lifo, |a, b| Ok(position(b) - position(a)))?;
                marks_to_open
                    .iter()
                    .map(|(kind, _)| *kind)
                    .chain(lifo)
                    .collect()
            } else {
                find_marks_to_close_at_end(&active_marks, &current_marks, next_node)?
            };

            let mut trailing_whitespace = String::new();
            if !marks_to_close_at_end.is_empty() {
                trailing_whitespace = split_trailing_whitespace(&mut text);
            }
            for kind in &marks_to_close_at_end {
                let mark = active_marks
                    .get(kind)
                    .or_else(|| current_marks.get(kind))
                    .copied();
                let mode = opening_modes.get(kind).copied().unwrap_or_default();
                text += &self.mark_markdown(Side::Closing, *kind, mark, mode)?;
                active_marks.shift_remove(kind);
                opening_modes.shift_remove(kind);
            }
            text += &trailing_whitespace;
            text += &middle_trailing_whitespace;
            result.push(text);
        }
        Ok(match <[String; 1]>::try_from(result) {
            Ok([only]) => only,
            Err(result) => result.join(separator),
        })
    }

    /// `getMarksToOpenForSerialization`: marks that end on this node open inside marks that
    /// continue into the next, and within each group lower ranks open outside.
    fn marks_to_open<'v>(
        &self,
        active_marks: &Marks,
        current_marks: &Marks<'v>,
        next_node: Option<&Value>,
    ) -> Result<Vec<(SameValueKey<'v>, &'v Value)>, Error> {
        let marks_to_open = find_marks_to_open(active_marks, current_marks);
        if marks_to_open.len() <= 1 {
            return Ok(marks_to_open);
        }
        let next_marks =
            value::or_empty_array(next_node.and_then(|next| value::optional(Some(next), "marks")));
        let continues_in_next_node =
            |(kind, mark): &(SameValueKey, &Value)| -> Result<bool, Error> {
                let attrs = value::optional(Some(mark), "attrs");
                let next_marks = value::array_method(next_marks, "nextMarks.some")?;
                for next_mark in next_marks {
                    if mark_type(next_mark)? == *kind
                        && attrs_equal(value::optional(Some(next_mark), "attrs"), attrs)
                    {
                        return Ok(true);
                    }
                }
                Ok(false)
            };
        let inner_first = |a: (SameValueKey, &Value), b: (SameValueKey, &Value)| {
            self.by_rank_inner_first(a.0, b.0)
        };
        let mut ending_here = Vec::new();
        for mark in &marks_to_open {
            if !continues_in_next_node(mark)? {
                ending_here.push(*mark);
            }
        }
        js::array::sort(&mut ending_here, inner_first)?;
        let mut continuing = Vec::new();
        for mark in &marks_to_open {
            if continues_in_next_node(mark)? {
                continuing.push(*mark);
            }
        }
        js::array::sort(&mut continuing, inner_first)?;
        ending_here.extend(continuing);
        Ok(ending_here)
    }

    /// `byRankInnerFirst`: lower ranks outside, and marks of the same rank, which only marks
    /// no extension renders share, by `a.type.localeCompare(b.type)`.
    fn by_rank_inner_first(&self, a: SameValueKey, b: SameValueKey) -> Result<f64, Error> {
        let rank = |kind: SameValueKey| {
            kind.as_str()
                .and_then(|kind| self.extension_ranks.get(kind))
                .map_or(js::MAX_SAFE_INTEGER, |rank| *rank as f64)
        };
        let (rank_a, rank_b) = (rank(a), rank(b));
        if rank_a != rank_b {
            return Ok(rank_b - rank_a);
        }
        match a {
            SameValueKey::String(a) => Ok(js::locale_compare(a, &b.to_js_string()?)),
            SameValueKey::Undefined => Err(value::cannot_read(Nullish::Undefined, "localeCompare")),
            SameValueKey::Null => Err(value::cannot_read(Nullish::Null, "localeCompare")),
            SameValueKey::Reference(_) | SameValueKey::Bool(_) | SameValueKey::Number(_) => {
                Err(value::not_a_function("a.type.localeCompare"))
            }
        }
    }

    /// `getMarkOpening` and `getMarkClosing`: the mark's Markdown before or after its content.
    fn mark_markdown(
        &self,
        side: Side,
        kind: SameValueKey,
        mark: Option<&Value>,
        mode: OpeningMode,
    ) -> Result<String, Error> {
        if mode == OpeningMode::Html {
            let (open, close) = self.html_reopen(kind).unwrap_or_default();
            return Ok(match side {
                Side::Opening => open,
                Side::Closing => close,
            }
            .to_string());
        }
        let Some((kind, render)) = kind.as_str().and_then(|kind| {
            let render = self.handler_for_node_type(kind)?.render.as_ref()?;
            Some((kind, render))
        }) else {
            return Ok(String::new());
        };
        let attrs = match value::get(mark, "attrs")? {
            Some(attrs) if js::truthy(Some(attrs)) => attrs.clone(),
            _ => Value::Object(Map::new()),
        };
        let node = json!({
            "type": kind,
            "attrs": attrs,
            "content": [{ "type": "text", "text": PLACEHOLDER }],
        });
        let context = RenderContext {
            previous_node: None,
        };
        let rendered = render(&node, &MarkHelpers, &context).map_err(|error| {
            Error::Other(match side {
                Side::Opening => format!("Failed to get mark opening for {kind}: {error}"),
                Side::Closing => format!("Failed to get mark closing for {kind}: {error}"),
            })
        })?;
        static FINDER: LazyLock<memchr::memmem::Finder> =
            LazyLock::new(|| memchr::memmem::Finder::new(PLACEHOLDER));
        Ok(match FINDER.find(rendered.as_bytes()) {
            Some(index) => match side {
                Side::Opening => &rendered[..index],
                Side::Closing => &rendered[index + PLACEHOLDER.len()..],
            }
            .to_string(),
            None => String::new(),
        })
    }

    /// `getHtmlReopenTags`.
    fn html_reopen(&self, kind: SameValueKey) -> Option<(&'static str, &'static str)> {
        self.handler_for_node_type(kind.as_str()?)?.html_reopen
    }
}

/// `markdownManager.isEmptyOutput`: whether the Markdown trims to nothing once its `&nbsp;`s and
/// U+00A0s go, which is whether it is only whitespace and `&nbsp;`s. U+00A0 is whitespace.
fn is_empty_output(markdown: &str) -> bool {
    let mut rest = markdown;
    loop {
        rest = js::trim_start(rest);
        match rest.strip_prefix("&nbsp;") {
            Some(after) => rest = after,
            None => return rest.is_empty(),
        }
    }
}

impl MarkdownManager {
    /// `encodeTextForMarkdown(node.text || "", node, parentNode)`: the text escaped for
    /// Markdown, or as it is inside code.
    fn encode_text_for_markdown(&self, node: &Value, parent: &Value) -> Result<String, Error> {
        let text = value::optional(Some(node), "text").filter(|text| js::truthy(Some(text)));
        if self.is_inside_code(node, parent)? {
            // The JS keeps text that isn't a string as it is, which joining it into the
            // Markdown makes the string it is here.
            return text.map_or(Ok(String::new()), js::to_string);
        }
        match text {
            Some(Value::String(text)) => Ok(escape_markdown(text)),
            Some(_) => Err(value::not_a_function("text.replace")),
            None => Ok(String::new()),
        }
    }

    /// `isInsideCode`: whether the parent or one of the text's marks is code.
    fn is_inside_code(&self, node: &Value, parent: &Value) -> Result<bool, Error> {
        let is_code = |kind: Option<&Value>| {
            kind.and_then(Value::as_str)
                .is_some_and(|kind| self.code_types.contains(kind))
        };
        if is_code(value::optional(Some(parent), "type")) {
            return Ok(true);
        }
        let marks = value::array_method(
            value::or_empty_array(value::optional(Some(node), "marks")),
            "(node.marks || []).some",
        )?;
        for mark in marks {
            let kind = match mark {
                Value::String(_) => Some(mark),
                _ => value::get(Some(mark), "type")?,
            };
            if is_code(kind) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// `escapeMarkdownSyntax(encodeHtmlEntities(text))`.
fn escape_markdown(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => encoded.push_str("&amp;"),
            '<' => encoded.push_str("&lt;"),
            '>' => encoded.push_str("&gt;"),
            '\\' | '`' | '*' | '_' | '[' | ']' | '~' => {
                encoded.push('\\');
                encoded.push(character);
            }
            other => encoded.push(other),
        }
    }
    encoded
}

/// Removes and returns `text`'s trailing whitespace, as `text.match(/(\s+)$/)` finds it.
fn split_trailing_whitespace(text: &mut String) -> String {
    let kept = js::trim_end(text).len();
    text.split_off(kept)
}

#[cfg(test)]
mod tests {
    #[test]
    fn empty_output_is_what_the_replacements_trim_to_nothing() {
        let alphabet = &[
            " ", "&nbsp;", "&", "nbsp;", "&nb", "sp;", "\u{A0}", "\u{3000}", "\n", "a",
        ];
        for units in tarnish_js::random::strings(alphabet, 50_000) {
            let markdown = String::from_utf16_lossy(&units);
            let replaced = markdown.replace("&nbsp;", "").replace('\u{A0}', "");
            assert_eq!(
                super::is_empty_output(&markdown),
                tarnish_js::trim(&replaced).is_empty(),
                "{markdown:?}"
            );
        }
    }
}
