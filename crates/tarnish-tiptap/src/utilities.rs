//! `@tiptap/core`'s utilities: comparing attributes and marks, HTML entities, and reading a
//! style property.

use tarnish::chunk::ValueRef;
use tarnish::json::Value;
use tarnish::{Mark, Node};
use tarnish_html::HtmlNode;

/// `node.attrs[name]`, of one of the node's type's attributes, which a node always holds.
pub fn node_attr<'a>(node: &'a Node, name: &str) -> ValueRef<'a> {
    node.attrs_view().get(name).expect("its type's attribute")
}

/// `mark.attrs[name]`, of one of the mark's type's attributes.
pub fn mark_attr<'a>(mark: &'a Mark, name: &str) -> ValueRef<'a> {
    mark.attrs_view().get(name).expect("its type's attribute")
}
use tarnish_js as js;
use tarnish_js::value;

/// `attrsEqual`: the same value, or shallowly the same own properties, compared with
/// `Object.is`, so nested objects are equal only if they are the same object.
pub fn attrs_equal(a: Option<&Value>, b: Option<&Value>) -> bool {
    if value::strict_equals(a, b) {
        return true;
    }
    match (a, b) {
        (Some(a), Some(b)) if js::truthy(Some(a)) && js::truthy(Some(b)) => {
            value::same_own_properties(a, b)
        }
        _ => false,
    }
}

/// `marksEqual`: the same marks, in any order.
pub fn marks_equal(a: &[Value], b: &[Value]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut consumed = vec![false; b.len()];
    a.iter().all(|mark| {
        let found = b.iter().enumerate().position(|(index, other)| {
            !consumed[index]
                && mark.get("type") == other.get("type")
                && attrs_equal(mark.get("attrs"), other.get("attrs"))
        });
        found.map(|index| consumed[index] = true).is_some()
    })
}

/// `decodeHtmlEntities`: `&lt;`, `&gt;`, `&quot;` and then `&amp;`, which decodes each entity
/// once, as one pass does.
pub fn decode_html_entities(text: String) -> String {
    if !text.contains('&') {
        return text;
    }
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(ampersand) = rest.find('&') {
        decoded.push_str(&rest[..ampersand]);
        rest = &rest[ampersand..];
        let entity = [
            ("&lt;", "<"),
            ("&gt;", ">"),
            ("&quot;", "\""),
            ("&amp;", "&"),
        ]
        .into_iter()
        .find(|(entity, _)| rest.starts_with(entity));
        let (length, character) =
            entity.map_or((1, "&"), |(entity, character)| (entity.len(), character));
        decoded.push_str(character);
        rest = &rest[length..];
    }
    decoded.push_str(rest);
    decoded
}

/// `getStyleProperty(element, property)`: the last value the `style` attribute gives the
/// property, matched without regard to case.
pub fn get_style_property(element: &HtmlNode, property: &str) -> Option<String> {
    let style = element
        .attribute("style")
        .filter(|style| !style.is_empty())?;
    let target = property.to_lowercase();
    style
        .split(';')
        .map(js::trim)
        .filter(|declaration| !declaration.is_empty())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .find_map(|declaration| {
            let (name, value) = declaration.split_once(':')?;
            (js::trim(name).to_lowercase() == target).then(|| js::trim(value).to_string())
        })
}
