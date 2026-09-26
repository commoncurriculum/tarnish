//! What HTML says about elements and whitespace.

use crate::dom::{Dom, NodeKind};
use crate::error::Result;

pub(super) const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "body",
    "canvas",
    "dd",
    "div",
    "dl",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "hr",
    "li",
    "noscript",
    "ol",
    "output",
    "p",
    "pre",
    "section",
    "table",
    "tfoot",
    "ul",
];

pub(super) const IGNORE_TAGS: &[&str] = &["head", "noscript", "object", "script", "style", "title"];

pub(super) fn is_list_tag(name: &str) -> bool {
    name == "ol" || name == "ul"
}

/// HTML's whitespace: space, tab, newline, carriage return and form feed.
pub(super) fn is_html_space(unit: u16) -> bool {
    matches!(unit, 0x20 | 0x09 | 0x0a | 0x0d | 0x0c)
}

/// Move lists that are directly inside lists into the item before them, as browsers take them.
pub(super) fn normalize_list<D: Dom>(dom: &D, list: &D::Node) -> Result<()> {
    let mut previous_item: Option<D::Node> = None;
    let mut child = dom.first_child(list)?;
    while let Some(current) = child {
        let name = match dom.kind(&current)? {
            NodeKind::Element => Some(dom.node_name(&current)?.to_lowercase()),
            _ => None,
        };
        let mut current = current;
        match name.as_deref() {
            Some(name) if is_list_tag(name) && previous_item.is_some() => {
                let item = previous_item.clone().expect("an item");
                dom.append_child(&item, &current)?;
                current = item;
            }
            Some("li") => previous_item = Some(current.clone()),
            Some(_) => previous_item = None,
            None => {}
        }
        child = dom.next_sibling(&current)?;
    }
    Ok(())
}

/// `value.replace(/[ \t\r\n\u000c]+/g, " ")`.
pub(super) fn collapse_spaces(value: &[u16]) -> Vec<u16> {
    let mut result = Vec::with_capacity(value.len());
    let mut in_space = false;
    for &unit in value {
        let space = is_html_space(unit);
        if !(space && in_space) {
            result.push(if space { 0x20 } else { unit });
        }
        in_space = space;
    }
    result
}

/// Line ends replaced with `with`: `\r\n`, `\r` and `\n` each.
pub(super) fn normalize_newlines(value: &[u16], with: &[u16]) -> Vec<u16> {
    let mut result = Vec::with_capacity(value.len());
    let mut index = 0;
    while index < value.len() {
        match value[index] {
            0x0d => {
                result.extend_from_slice(with);
                if value.get(index + 1) == Some(&0x0a) {
                    index += 1;
                }
            }
            0x0a => result.extend_from_slice(with),
            unit => result.push(unit),
        }
        index += 1;
    }
    result
}

/// `value.split(/\r?\n|\r/)`.
pub(super) fn split_lines(value: &[u16]) -> Vec<Vec<u16>> {
    let mut lines = vec![Vec::new()];
    let mut index = 0;
    while index < value.len() {
        match value[index] {
            0x0d => {
                if value.get(index + 1) == Some(&0x0a) {
                    index += 1;
                }
                lines.push(Vec::new());
            }
            0x0a => lines.push(Vec::new()),
            unit => lines.last_mut().expect("a line").push(unit),
        }
        index += 1;
    }
    lines
}
