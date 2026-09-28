//! Writing HTML as <https://html.spec.whatwg.org/#serialising-html-fragments> does, with
//! scripting off.

use std::borrow::Cow;

use html5ever::QualName;

use crate::tree::{Data, Element, NodeId, Space, Tree};

/// Whether an HTML element of this local name is void: written without content or an end tag.
pub fn is_void(local: &str) -> bool {
    const VOID: [&str; 18] = [
        "area", "base", "basefont", "bgsound", "br", "col", "embed", "frame", "hr", "img", "input",
        "keygen", "link", "meta", "param", "source", "track", "wbr",
    ];
    VOID.contains(&local)
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

/// Write an attribute's value, escaping `&`, `"` and U+00A0.
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
            b'"' if attribute => ("&quot;", 1),
            b'<' if !attribute => ("&lt;", 1),
            b'>' if !attribute => ("&gt;", 1),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_and_attributes_escape_what_the_standard_escapes() {
        let mut out = String::new();
        escape_text(&mut out, "a<b>&\"c\u{a0}é\u{120}");
        out.push('|');
        escape_attribute(&mut out, "a<b>&\"c\u{a0}é\u{120}");
        assert_eq!(
            out,
            "a&lt;b&gt;&amp;\"c&nbsp;é\u{120}|a<b>&amp;&quot;c&nbsp;é\u{120}"
        );
    }

    #[test]
    fn attribute_names_take_their_namespaces_prefixes() {
        let xlink = Some("http://www.w3.org/1999/xlink");
        let xmlns = Some("http://www.w3.org/2000/xmlns/");
        assert_eq!(attribute_name(None, None, "href"), "href");
        assert_eq!(attribute_name(xlink, Some("ns1"), "href"), "xlink:href");
        assert_eq!(attribute_name(xmlns, None, "xmlns"), "xmlns");
        assert_eq!(attribute_name(xmlns, Some("xmlns"), "a"), "xmlns:a");
        assert_eq!(attribute_name(Some("urn:x"), Some("p"), "r"), "p:r");
        assert_eq!(attribute_name(Some("urn:x"), None, "r"), "r");
    }
}
