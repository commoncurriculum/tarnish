//! Parsing documents from a DOM, and serializing them to one: prosemirror-model's `DOMParser`
//! and `DOMSerializer`, over any DOM that implements [`Dom`].

mod parse;
mod serialize;

pub use parse::{
    AttrsHook, ClearMarkHook, Content, ContentElement, ContentElementHook, DomParser, ElementRule,
    FindPosition, GetAttrsResult, GetContentHook, Namespace, ParseOptions, PreserveWhitespace,
    Rule, RuleFromNode, Skip, StyleAttrsHook, StyleRule, TagRule, by_priority,
};
pub use serialize::{DomSerializer, DomSpec, MarkToDom, NodeToDom, Rendered, render_spec};

use crate::error::Result;
use crate::json::Value;
use crate::text::Text;

/// What kind of DOM node a node is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Element,
    Text,
    Other,
}

/// A DOM, as the parser reads it and the serializer writes it: a browser's, or any tree that
/// answers the same questions. Its nodes are handles, and every call may fail with the host's
/// error.
pub trait Dom {
    type Node: Clone;

    fn kind(&self, node: &Self::Node) -> Result<NodeKind>;
    /// `nodeName`: an HTML element's tag name in upper case, `#text` for text.
    fn node_name(&self, node: &Self::Node) -> Result<String>;
    /// A text node's text.
    fn text(&self, node: &Self::Node) -> Result<Text>;
    fn namespace(&self, node: &Self::Node) -> Result<Option<String>>;
    fn parent(&self, node: &Self::Node) -> Result<Option<Self::Node>>;
    fn first_child(&self, node: &Self::Node) -> Result<Option<Self::Node>>;
    fn next_sibling(&self, node: &Self::Node) -> Result<Option<Self::Node>>;
    fn previous_sibling(&self, node: &Self::Node) -> Result<Option<Self::Node>>;
    /// `childNodes[index]`.
    fn child(&self, node: &Self::Node, index: usize) -> Result<Option<Self::Node>>;
    /// Whether the element matches a CSS selector.
    fn matches(&self, node: &Self::Node, selector: &str) -> Result<bool>;
    fn query_selector(&self, node: &Self::Node, selector: &str) -> Result<Option<Self::Node>>;
    /// How many properties the element's inline style sets.
    fn style_count(&self, node: &Self::Node) -> Result<usize>;
    /// `style.getPropertyValue(property)`: empty when the style doesn't set it.
    fn style_value(&self, node: &Self::Node, property: &str) -> Result<String>;
    /// Whether `a` and `b` are the same node.
    fn same(&self, a: &Self::Node, b: &Self::Node) -> Result<bool>;
    /// Whether `node` is `ancestor` or inside it.
    fn contains(&self, ancestor: &Self::Node, node: &Self::Node) -> Result<bool>;
    /// `a.compareDocumentPosition(b)`'s bits.
    fn compare_document_position(&self, a: &Self::Node, b: &Self::Node) -> Result<u16>;

    fn append_child(&self, parent: &Self::Node, child: &Self::Node) -> Result<()>;
    fn create_element(&self, namespace: Option<&str>, name: &str) -> Result<Self::Node>;
    fn create_text(&self, text: &Text) -> Result<Self::Node>;
    fn create_fragment(&self) -> Result<Self::Node>;
    /// Set an attribute to a value, as `setAttribute` or `setAttributeNS` turns it into a
    /// string.
    fn set_attribute(
        &self,
        element: &Self::Node,
        namespace: Option<&str>,
        name: &str,
        value: &Value,
    ) -> Result<()>;
    /// Set the element's inline style from CSS text. `false` when the element has no style to
    /// set, for the attribute to be set instead.
    fn set_style(&self, element: &Self::Node, css: &Value) -> Result<bool>;
    /// `String(node)`: what an attribute set to the node holds.
    fn stringify(&self, node: &Self::Node) -> Result<String>;
}
