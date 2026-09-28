//! Writing HTML as jsdom 20's `innerHTML` and `outerHTML` do, with parse5 7's serializer and
//! scripting off.

use html5ever::ns;

use crate::tree::{Data, Element, NodeId, Tree, qualified};

const VOID: [&str; 18] = [
    "area", "base", "basefont", "bgsound", "br", "col", "embed", "frame", "hr", "img", "input",
    "keygen", "link", "meta", "param", "source", "track", "wbr",
];

/// The elements whose text is written as it is. With scripting on, `noscript` would be one.
const RAW_TEXT: [&str; 7] = [
    "style",
    "script",
    "xmp",
    "iframe",
    "noembed",
    "noframes",
    "plaintext",
];

fn is_void(element: &Element) -> bool {
    element.is_html() && VOID.contains(&element.qualified_name().as_str())
}

/// `innerHTML`: the node's children, or a template's content.
pub(crate) fn inner(tree: &Tree, node: NodeId) -> String {
    let mut out = String::new();
    if tree.element(node).is_some_and(is_void) {
        return out;
    }
    for child in children(tree, node) {
        write(tree, child, &mut out);
    }
    out
}

/// `outerHTML`: the node with its children.
pub(crate) fn outer(tree: &Tree, node: NodeId) -> String {
    let mut out = String::new();
    write(tree, node, &mut out);
    out
}

/// The children a node's HTML holds: a template's are its content's.
fn children(tree: &Tree, node: NodeId) -> impl Iterator<Item = NodeId> + '_ {
    let element = tree.element(node);
    let container = match element.and_then(|element| element.template_contents) {
        Some(contents) if element.is_some_and(|element| element.is_html()) => contents,
        _ => node,
    };
    tree.children(container)
}

enum Step {
    Open(NodeId),
    Close(NodeId),
}

fn write(tree: &Tree, node: NodeId, out: &mut String) {
    let mut steps = vec![Step::Open(node)];
    while let Some(step) = steps.pop() {
        let node = match step {
            Step::Open(node) => node,
            Step::Close(node) => {
                let element = tree.element(node).expect("an element");
                out.push_str("</");
                out.push_str(&element.qualified_name());
                out.push('>');
                continue;
            }
        };
        match &tree.node(node).data {
            Data::Element(element) => {
                out.push('<');
                out.push_str(&element.qualified_name());
                for attr in &element.attrs {
                    out.push(' ');
                    out.push_str(&qualified(&attr.name));
                    out.push_str("=\"");
                    escape(&attr.value, true, out);
                    out.push('"');
                }
                out.push('>');
                if !is_void(element) {
                    steps.push(Step::Close(node));
                    let start = steps.len();
                    steps.extend(children(tree, node).map(Step::Open));
                    steps[start..].reverse();
                }
            }
            Data::Text(text) => {
                let parent = tree
                    .node(node)
                    .parent
                    .and_then(|parent| tree.element(parent));
                let raw = parent.is_some_and(|parent| {
                    parent.name.ns == ns!(html)
                        && RAW_TEXT.contains(&parent.qualified_name().as_str())
                });
                match raw {
                    true => out.push_str(text),
                    false => escape(text, false, out),
                }
            }
            Data::Comment(comment) => {
                out.push_str("<!--");
                out.push_str(comment);
                out.push_str("-->");
            }
            Data::Doctype { name } => {
                out.push_str("<!DOCTYPE ");
                out.push_str(name);
                out.push('>');
            }
            Data::Document | Data::Fragment | Data::ProcessingInstruction { .. } => {}
        }
    }
}

/// Escape text, or an attribute's value, which escapes quotes where text escapes angle
/// brackets.
fn escape(text: &str, attribute: bool, out: &mut String) {
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            '"' if attribute => out.push_str("&quot;"),
            '<' if !attribute => out.push_str("&lt;"),
            '>' if !attribute => out.push_str("&gt;"),
            _ => out.push(character),
        }
    }
}
