//! What HTML says about elements and whitespace.

use crate::Result;
use crate::Text;
use crate::dom::{Dom, NodeKind};

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
    is_tag(&["ol", "ul"], name)
}

/// Whether `name` in lower case, as JavaScript's `toLowerCase` makes it, is one of the tags.
pub(super) fn is_tag(tags: &[&str], name: &str) -> bool {
    match name.is_ascii() {
        true => tags.iter().any(|tag| tag.eq_ignore_ascii_case(name)),
        false => tags.contains(&name.to_lowercase().as_str()),
    }
}

/// Move lists that are directly inside lists into the item before them, as browsers take them.
pub(super) fn normalize_list<D: Dom>(dom: &D, list: &D::Node) -> Result<()> {
    let mut previous_item: Option<D::Node> = None;
    let mut child = dom.first_child(list)?;
    while let Some(mut current) = child {
        let name = match dom.kind(&current)? {
            NodeKind::Element => Some(dom.node_name(&current)?.to_lowercase()),
            _ => None,
        };
        match (name.as_deref(), &previous_item) {
            (Some(name), Some(item)) if is_list_tag(name) => {
                dom.append_child(item, &current)?;
                current = item.clone();
            }
            (Some("li"), _) => previous_item = Some(current.clone()),
            (Some(_), _) => previous_item = None,
            (None, _) => {}
        }
        child = dom.next_sibling(&current)?;
    }
    Ok(())
}

/// HTML's whitespace: space, tab, newline, carriage return and form feed.
pub(super) fn is_html_space(unit: u16) -> bool {
    matches!(unit, 0x20 | 0x09 | 0x0a | 0x0d | 0x0c)
}

/// Whether the text holds anything but HTML whitespace: `/[^ \t\r\n\u000c]/`.
pub(super) fn has_non_space(text: &Text) -> bool {
    text.held_units().any(|unit| !is_html_space(unit))
}

/// The length of the HTML whitespace the text ends in.
pub(super) fn trailing_spaces(text: &Text) -> usize {
    let units = text.held_units().rev();
    units.take_while(|&unit| is_html_space(unit)).count()
}

/// `text.replace(/[ \t\r\n\u000c]+/g, " ")`.
pub(super) fn collapse_spaces(text: &Text) -> Text {
    let mut after_space = false;
    let changes = text.held_units().any(|unit| {
        let space = is_html_space(unit);
        let changes = space && (unit != 0x20 || after_space);
        after_space = space;
        changes
    });
    if !changes {
        return text.clone();
    }
    match text.as_str() {
        Some(text) => {
            let space = |character: char| character.is_ascii() && is_html_space(character as u16);
            Text::from(collapse(text.chars(), ' ', space).collect::<String>())
        }
        None => {
            let units = text.units();
            let collapsed = collapse(units.iter().copied(), 0x20, is_html_space);
            Text::from_units(&collapsed.collect::<Vec<_>>())
        }
    }
}

/// The items with each run of those `space` picks as one `with`.
fn collapse<T: Copy>(
    items: impl Iterator<Item = T>,
    with: T,
    space: impl Fn(T) -> bool,
) -> impl Iterator<Item = T> {
    let mut after_space = false;
    items.filter_map(move |item| {
        let is_space = space(item);
        let keep = !(is_space && after_space);
        after_space = is_space;
        keep.then_some(if is_space { with } else { item })
    })
}

#[cfg(test)]
mod tests {
    use super::{collapse_spaces, has_non_space, is_html_space, trailing_spaces};
    use crate::Text;

    /// Collapsing and measuring HTML whitespace agree with a scan of the units, for text held
    /// as UTF-8 and for text with a lone surrogate, held as units.
    #[test]
    fn scans_whitespace_as_units() {
        let texts = [
            Text::from(" a  b\t\r\n😀 \u{a0} é\u{c} "),
            Text::from("a b"),
            Text::from("  "),
            Text::from(""),
            Text::from_units(&[0x20, 0x20, 0xd800, 0x09, 0x61, 0x0a]),
        ];
        for text in texts {
            let units = text.units().into_owned();
            let mut collapsed: Vec<u16> = Vec::new();
            for &unit in &units {
                if !is_html_space(unit) {
                    collapsed.push(unit);
                } else if collapsed.last() != Some(&0x20) {
                    collapsed.push(0x20);
                }
            }
            assert_eq!(
                collapse_spaces(&text),
                Text::from_units(&collapsed),
                "{text:?}"
            );
            let trailing = units.iter().rev().take_while(|&&unit| is_html_space(unit));
            assert_eq!(trailing_spaces(&text), trailing.count(), "{text:?}");
            let non_space = units.iter().any(|&unit| !is_html_space(unit));
            assert_eq!(has_non_space(&text), non_space, "{text:?}");
        }
    }
}
