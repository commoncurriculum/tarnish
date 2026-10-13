//! Writing HTML as <https://html.spec.whatwg.org/#serialising-html-fragments> does, with
//! scripting off.

use std::borrow::Cow;

use html5ever::QualName;

use crate::tree::{Data, Element, NodeId, Space, Tree};

/// Whether an HTML element of this local name is void: written without content or an end tag.
pub fn is_void(local: &str) -> bool {
    matches!(
        local,
        "area"
            | "base"
            | "basefont"
            | "bgsound"
            | "br"
            | "col"
            | "embed"
            | "frame"
            | "hr"
            | "img"
            | "input"
            | "keygen"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Whether the text in an HTML element of this local name is written as it is. With scripting
/// on, `noscript` would be one.
pub fn is_raw_text(local: &str) -> bool {
    matches!(
        local,
        "style" | "script" | "xmp" | "iframe" | "noembed" | "noframes" | "plaintext"
    )
}

/// The name an attribute is written with: its namespace's usual prefix for XML's, XMLNS's and
/// XLink's, its own in any other namespace, and then its local name.
pub fn attribute_name<'a>(
    namespace: Option<&str>,
    prefix: Option<&'a str>,
    local: &'a str,
) -> Cow<'a, str> {
    match namespace {
        None => Cow::Borrowed(local),
        Some("http://www.w3.org/XML/1998/namespace") => Cow::Owned(format!("xml:{local}")),
        Some("http://www.w3.org/2000/xmlns/") if local == "xmlns" => Cow::Borrowed(local),
        Some("http://www.w3.org/2000/xmlns/") => Cow::Owned(format!("xmlns:{local}")),
        Some("http://www.w3.org/1999/xlink") => Cow::Owned(format!("xlink:{local}")),
        Some(_) => match prefix {
            Some(prefix) => Cow::Owned(format!("{prefix}:{local}")),
            None => Cow::Borrowed(local),
        },
    }
}

/// Write text, escaping `&`, `<`, `>` and U+00A0.
pub fn escape_text(out: &mut String, text: &str) {
    escape(out, text, false);
}

/// Write an attribute's value, escaping `&`, `"`, `<`, `>` and U+00A0. The standard escapes
/// `<` and `>` in attributes since <https://github.com/whatwg/html/pull/6362>.
pub fn escape_attribute(out: &mut String, value: &str) {
    escape(out, value, true);
}

fn escape(out: &mut String, text: &str, attribute: bool) {
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        let (escaped, length) = match bytes[index] {
            b'&' => ("&amp;", 1),
            b'<' => ("&lt;", 1),
            b'>' => ("&gt;", 1),
            b'"' if attribute => ("&quot;", 1),
            0xC2 if bytes.get(index + 1) == Some(&0xA0) => ("&nbsp;", 2),
            _ => {
                index += 1;
                continue;
            }
        };
        out.push_str(&text[start..index]);
        out.push_str(escaped);
        index += length;
        start = index;
    }
    out.push_str(&text[start..]);
}

fn write_tag(out: &mut String, element: &Element) {
    if let (Space::Other, Some(prefix)) = (element.space(), &element.name.prefix) {
        out.push_str(prefix);
        out.push(':');
    }
    out.push_str(&element.name.local);
}

fn write_attribute_name(out: &mut String, name: &QualName) {
    let namespace = Some(&*name.ns).filter(|namespace| !namespace.is_empty());
    out.push_str(&attribute_name(
        namespace,
        name.prefix.as_deref(),
        &name.local,
    ));
}

fn is_void_element(element: &Element) -> bool {
    element.is_html() && is_void(&element.name.local)
}

/// `innerHTML`: the node's children, or a template's content.
pub(crate) fn inner(tree: &Tree, node: NodeId) -> String {
    let mut out = String::new();
    if tree.element(node).is_some_and(is_void_element) {
        return out;
    }
    for child in children(tree, node) {
        write(tree, child, &mut out, None);
    }
    out
}

/// `outerHTML`: the node with its children.
pub(crate) fn outer(tree: &Tree, node: NodeId) -> String {
    let mut out = String::new();
    write(tree, node, &mut out, None);
    out
}

/// `outerHTML` of `node`, split where a node appended to the element `at` inside it would be
/// written: what comes before, and what comes after.
pub(crate) fn outer_split(tree: &Tree, node: NodeId, at: NodeId) -> (String, String) {
    let mut out = String::new();
    let split = write(tree, node, &mut out, Some(at)).unwrap_or(out.len());
    let after = out.split_off(split);
    (out, after)
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

/// Writes the node, and gives where a node appended to `split_at` would be written.
fn write(tree: &Tree, node: NodeId, out: &mut String, split_at: Option<NodeId>) -> Option<usize> {
    let mut split = None;
    let mut steps = vec![Step::Open(node)];
    while let Some(step) = steps.pop() {
        let node = match step {
            Step::Open(node) => node,
            Step::Close(node) => {
                if split_at == Some(node) {
                    split = Some(out.len());
                }
                let element = tree.element(node).expect("an element");
                out.push_str("</");
                write_tag(out, element);
                out.push('>');
                continue;
            }
        };
        match &tree.node(node).data {
            Data::Element(element) => {
                out.push('<');
                write_tag(out, element);
                for attr in &element.attrs {
                    out.push(' ');
                    write_attribute_name(out, &attr.name);
                    out.push_str("=\"");
                    escape_attribute(out, &attr.value);
                    out.push('"');
                }
                out.push('>');
                if !is_void_element(element) {
                    steps.push(Step::Close(node));
                    let start = steps.len();
                    steps.extend(children(tree, node).map(Step::Open));
                    steps[start..].reverse();
                } else if split_at == Some(node) {
                    split = Some(out.len());
                }
            }
            Data::Text(text) => {
                let parent = tree
                    .node(node)
                    .parent
                    .and_then(|parent| tree.element(parent));
                match parent
                    .is_some_and(|parent| parent.is_html() && is_raw_text(&parent.name.local))
                {
                    true => out.push_str(text),
                    false => escape_text(out, text),
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
            Data::Document | Data::Fragment => {}
        }
    }
    split
}
