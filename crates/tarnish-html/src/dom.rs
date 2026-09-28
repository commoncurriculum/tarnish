//! The DOM: nodes in an arena that handles share, and the `Dom` the parser and serializer
//! read and write it through.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use html5ever::{LocalName, Namespace, Prefix, QualName, local_name, ns};
use tarnish::dom::{Dom, NodeKind};
use tarnish::{Error, Result, Text, Value, js};

use crate::interface::interface;
use crate::names::{self, NameKind};
use crate::select::{self, Selectors};
use crate::serialize;
use crate::tree::{Attr, Data, Element, FOLLOWING, NodeId, PRECEDING, Space, Tree};
use tarnish_css::Declarations;

/// An HTML document and the nodes made in it: what [`parse_document`](HtmlDom::parse_document)
/// parsed, the fragments [`parse_fragment`](HtmlDom::parse_fragment) parsed, and what a
/// [`DomSerializer`](tarnish::dom::DomSerializer) writes into it.
///
/// It is the [`Dom`] that a [`DomParser`](tarnish::dom::DomParser) reads and a
/// [`DomSerializer`](tarnish::dom::DomSerializer) writes. Its nodes are [`HtmlNode`]s, which
/// reach the DOM they are in, so a rule's `getAttrs` can read an element's attributes.
///
/// - **Lifetime.** The DOM lives while a handle to it or to one of its nodes does, and keeps
///   every node made in it until then, so make one per document rather than one per server.
/// - **Threads.** DOMs and nodes are `Send` and `Sync`, and so is a `DomParser<HtmlNode>`.
/// - **Quirks mode.** A document parsed in quirks mode makes class and id selectors ignore case
///   for every node of the DOM.
#[derive(Clone)]
pub struct HtmlDom {
    shared: Arc<Shared>,
}

struct Shared {
    tree: Mutex<Tree>,
    selectors: Mutex<HashMap<String, Option<Arc<Selectors>>>>,
}

/// A node of an [`HtmlDom`]. Handles to the same node are equal.
#[derive(Clone)]
pub struct HtmlNode {
    dom: HtmlDom,
    id: NodeId,
}

const DOCUMENT: NodeId = 0;

impl Default for HtmlDom {
    fn default() -> Self {
        HtmlDom::new()
    }
}

impl HtmlDom {
    /// A DOM with an empty document, to make nodes in.
    pub fn new() -> HtmlDom {
        let mut tree = Tree::default();
        tree.push(Data::Document);
        HtmlDom::from_tree(tree)
    }

    /// Parse a whole document, as `DOMParser.parseFromString(html, "text/html")` does.
    pub fn parse_document(html: &str) -> HtmlDom {
        let mut tree = Tree::default();
        crate::parse::document(&mut tree, html);
        HtmlDom::from_tree(tree)
    }

    fn from_tree(tree: Tree) -> HtmlDom {
        HtmlDom {
            shared: Arc::new(Shared {
                tree: Mutex::new(tree),
                selectors: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Parse HTML into a document fragment in this DOM, as a `<template>`'s content holds it:
    /// any element stays where it is, a `<td>` or a `<tr>` as much as a `<p>`.
    pub fn parse_fragment(&self, html: &str) -> HtmlNode {
        let id = crate::parse::fragment(&mut self.tree(), html);
        self.node(id)
    }

    /// The document node.
    pub fn document(&self) -> HtmlNode {
        self.node(DOCUMENT)
    }

    /// `document.body`: the document element's first `<body>` or `<frameset>` child.
    pub fn body(&self) -> Option<HtmlNode> {
        let tree = self.tree();
        let root = tree
            .children(DOCUMENT)
            .find(|&child| tree.element(child).is_some())?;
        let root_element = tree.element(root)?;
        if root_element.name != QualName::new(None, ns!(html), local_name!("html")) {
            return None;
        }
        let body = tree.children(root).find(|&child| {
            tree.element(child).is_some_and(|element| {
                element.is_html()
                    && matches!(
                        element.name.local,
                        local_name!("body") | local_name!("frameset")
                    )
            })
        })?;
        drop(tree);
        Some(self.node(body))
    }

    fn node(&self, id: NodeId) -> HtmlNode {
        HtmlNode {
            dom: self.clone(),
            id,
        }
    }

    fn tree(&self) -> MutexGuard<'_, Tree> {
        self.shared
            .tree
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn selectors(&self, selector: &str) -> Result<Arc<Selectors>> {
        let mut cache = self
            .shared
            .selectors
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let parsed = match cache.get(selector) {
            Some(parsed) => parsed.clone(),
            None => {
                let parsed = select::parse(selector).map(Arc::new);
                cache.insert(selector.to_owned(), parsed.clone());
                parsed
            }
        };
        parsed.ok_or_else(|| Error::Syntax(format!("'{selector}' is not a valid selector")))
    }

    /// The node's id, when it is one of this DOM's.
    fn own(&self, node: &HtmlNode) -> Result<NodeId> {
        match Arc::ptr_eq(&self.shared, &node.dom.shared) {
            true => Ok(node.id),
            false => Err(Error::Other(
                "WrongDocumentError: The node is in another HtmlDom".into(),
            )),
        }
    }

    fn element_of(&self, node: &HtmlNode) -> Result<NodeId> {
        let id = self.own(node)?;
        match self.tree().element(id) {
            Some(_) => Ok(id),
            None => Err(Error::Type("The node is not an element".into())),
        }
    }

    /// Run `read` on the element's inline style, when it can have one.
    fn with_style<T>(node: &HtmlNode, read: impl FnOnce(&Declarations) -> T, none: T) -> T {
        let mut tree = node.dom.tree();
        let Some(element) = tree
            .element_mut(node.id)
            .filter(|element| element.space() != Space::Other)
        else {
            return none;
        };
        let style = match element.style {
            Some(ref style) => style,
            None => {
                let css = element.attr_ns(&ns!(), &local_name!("style"));
                let style = Declarations::parse(css.map_or("", |attr| &attr.value));
                element.style.insert(style)
            }
        };
        read(style)
    }
}

impl HtmlNode {
    /// The DOM the node is in.
    pub fn dom(&self) -> &HtmlDom {
        &self.dom
    }

    /// `innerHTML`: the HTML of the node's children, or of a template's content.
    pub fn inner_html(&self) -> String {
        serialize::inner(&self.dom.tree(), self.id)
    }

    /// `outerHTML`: the HTML of the node and its children.
    pub fn outer_html(&self) -> String {
        serialize::outer(&self.dom.tree(), self.id)
    }

    /// `getAttribute`: the value of the attribute of this qualified name, which an HTML
    /// element looks for in lower case.
    pub fn attribute(&self, name: &str) -> Option<String> {
        let tree = self.dom.tree();
        tree.element(self.id)?.attribute(name).map(str::to_owned)
    }

    /// `hasAttribute`.
    pub fn has_attribute(&self, name: &str) -> bool {
        self.attribute(name).is_some()
    }

    /// `style.getPropertyValue(property)`: the value the inline style gives the property, empty
    /// when it gives none. See [`Dom::style_value`].
    pub fn style_value(&self, property: &str) -> String {
        HtmlDom::with_style(self, |style| style.value(property), String::new())
    }

    /// `nodeName`: an HTML element's qualified name in upper case, `#text` for text, and so on.
    pub fn node_name(&self) -> String {
        node_name(&self.dom.tree(), self.id)
    }
}

impl PartialEq for HtmlNode {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.dom.shared, &other.dom.shared) && self.id == other.id
    }
}

impl Eq for HtmlNode {}

impl fmt::Debug for HtmlNode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "HtmlNode({} {})", self.id, self.node_name())
    }
}

fn node_name(tree: &Tree, id: NodeId) -> String {
    match &tree.node(id).data {
        Data::Element(element) if element.is_html() => {
            element.qualified_name().to_ascii_uppercase()
        }
        Data::Element(element) => element.qualified_name(),
        Data::Text(_) => "#text".into(),
        Data::Comment(_) => "#comment".into(),
        Data::Document => "#document".into(),
        Data::Fragment => "#document-fragment".into(),
        Data::Doctype { name } => name.clone(),
        Data::ProcessingInstruction { target, .. } => target.clone(),
    }
}

fn qual_name((namespace, prefix, local): (Option<&str>, Option<&str>, &str)) -> QualName {
    QualName::new(
        prefix.map(Prefix::from),
        namespace.map_or(ns!(), Namespace::from),
        LocalName::from(local),
    )
}

fn hierarchy_error(message: &str) -> Error {
    Error::Other(format!("HierarchyRequestError: {message}"))
}

impl Dom for HtmlDom {
    type Node = HtmlNode;

    fn kind(&self, node: &HtmlNode) -> Result<NodeKind> {
        Ok(match node.dom.tree().node(node.id).data {
            Data::Element(_) => NodeKind::Element,
            Data::Text(_) => NodeKind::Text,
            _ => NodeKind::Other,
        })
    }

    fn node_name(&self, node: &HtmlNode) -> Result<String> {
        Ok(node.node_name())
    }

    fn text(&self, node: &HtmlNode) -> Result<Text> {
        Ok(match &node.dom.tree().node(node.id).data {
            Data::Text(text) | Data::Comment(text) => Text::from(text.as_str()),
            Data::ProcessingInstruction { data, .. } => Text::from(data.as_str()),
            _ => Text::default(),
        })
    }

    fn namespace(&self, node: &HtmlNode) -> Result<Option<String>> {
        let tree = node.dom.tree();
        let namespace = tree.element(node.id).map(|element| &element.name.ns);
        Ok(namespace
            .filter(|namespace| **namespace != ns!())
            .map(|namespace| namespace.to_string()))
    }

    fn parent(&self, node: &HtmlNode) -> Result<Option<HtmlNode>> {
        let parent = node.dom.tree().node(node.id).parent;
        Ok(parent.map(|parent| node.dom.node(parent)))
    }

    fn first_child(&self, node: &HtmlNode) -> Result<Option<HtmlNode>> {
        let first = node.dom.tree().node(node.id).first;
        Ok(first.map(|first| node.dom.node(first)))
    }

    fn next_sibling(&self, node: &HtmlNode) -> Result<Option<HtmlNode>> {
        let next = node.dom.tree().node(node.id).next;
        Ok(next.map(|next| node.dom.node(next)))
    }

    fn previous_sibling(&self, node: &HtmlNode) -> Result<Option<HtmlNode>> {
        let previous = node.dom.tree().node(node.id).previous;
        Ok(previous.map(|previous| node.dom.node(previous)))
    }

    fn child(&self, node: &HtmlNode, index: usize) -> Result<Option<HtmlNode>> {
        let child = node.dom.tree().children(node.id).nth(index);
        Ok(child.map(|child| node.dom.node(child)))
    }

    fn matches(&self, node: &HtmlNode, selector: &str) -> Result<bool> {
        let selectors = node.dom.selectors(selector)?;
        Ok(select::matches(
            &node.dom.tree(),
            node.id,
            &selectors,
            node.id,
        ))
    }

    fn query_selector(&self, node: &HtmlNode, selector: &str) -> Result<Option<HtmlNode>> {
        let selectors = node.dom.selectors(selector)?;
        let found = select::query(&node.dom.tree(), node.id, &selectors);
        Ok(found.map(|found| node.dom.node(found)))
    }

    fn style_count(&self, node: &HtmlNode) -> Result<usize> {
        Ok(HtmlDom::with_style(node, Declarations::len, 0))
    }

    fn style_value(&self, node: &HtmlNode, property: &str) -> Result<String> {
        Ok(node.style_value(property))
    }

    fn same(&self, a: &HtmlNode, b: &HtmlNode) -> Result<bool> {
        Ok(a == b)
    }

    fn contains(&self, ancestor: &HtmlNode, node: &HtmlNode) -> Result<bool> {
        Ok(Arc::ptr_eq(&ancestor.dom.shared, &node.dom.shared)
            && ancestor.dom.tree().contains(ancestor.id, node.id))
    }

    fn compare_document_position(&self, a: &HtmlNode, b: &HtmlNode) -> Result<u16> {
        if !Arc::ptr_eq(&a.dom.shared, &b.dom.shared) {
            let (a, b) = (Arc::as_ptr(&a.dom.shared), Arc::as_ptr(&b.dom.shared));
            let order = if b < a { PRECEDING } else { FOLLOWING };
            return Ok(0x01 | 0x20 | order);
        }
        Ok(a.dom.tree().compare_document_position(a.id, b.id))
    }

    fn append_child(&self, parent: &HtmlNode, child: &HtmlNode) -> Result<()> {
        let (parent_id, child_id) = (self.own(parent)?, self.own(child)?);
        let mut tree = self.tree();
        let elements = |tree: &Tree, id: NodeId| {
            let children = tree.children(id);
            children
                .filter(|&child| tree.element(child).is_some())
                .count()
        };
        let into_document = match tree.node(parent_id).data {
            Data::Document => true,
            Data::Fragment | Data::Element(_) => false,
            _ => return Err(hierarchy_error("The parent can't have children")),
        };
        if tree.contains(child_id, parent_id) {
            return Err(hierarchy_error("The new child contains the parent"));
        }
        let (is_fragment, adds_elements) = match tree.node(child_id).data {
            Data::Document => {
                return Err(hierarchy_error("A document can't be a child"));
            }
            Data::Text(_) if into_document => {
                return Err(hierarchy_error("A document can't hold text"));
            }
            Data::Doctype { .. } if !into_document => {
                return Err(hierarchy_error("Only a document can hold a doctype"));
            }
            Data::Element(_) => (false, 1),
            Data::Fragment => (true, elements(&tree, child_id)),
            _ => (false, 0),
        };
        if into_document && elements(&tree, parent_id) + adds_elements > 1 {
            return Err(hierarchy_error("A document has only one element"));
        }
        match is_fragment {
            true => tree.reparent_children(child_id, parent_id),
            false => tree.append(parent_id, child_id),
        }
        Ok(())
    }

    fn create_element(&self, namespace: Option<&str>, name: &str) -> Result<HtmlNode> {
        let name = match namespace {
            None => {
                NameKind::Element.validate(name)?;
                let local = LocalName::from(name.to_ascii_lowercase());
                QualName::new(None, ns!(html), local)
            }
            Some(namespace) => {
                let name = names::validate_and_extract(namespace, name, NameKind::Element)?;
                qual_name(name)
            }
        };
        let mut tree = self.tree();
        let is_template = name.ns == ns!(html) && name.local == local_name!("template");
        let mut element = Element::new(name, Vec::new());
        if is_template {
            element.template_contents = Some(tree.push(Data::Fragment));
        }
        let id = tree.push(Data::Element(Box::new(element)));
        drop(tree);
        Ok(self.node(id))
    }

    fn create_text(&self, text: &Text) -> Result<HtmlNode> {
        let id = self
            .tree()
            .push(Data::Text(text.to_string_lossy().into_owned()));
        Ok(self.node(id))
    }

    fn create_fragment(&self) -> Result<HtmlNode> {
        let id = self.tree().push(Data::Fragment);
        Ok(self.node(id))
    }

    fn set_attribute(
        &self,
        element: &HtmlNode,
        namespace: Option<&str>,
        name: &str,
        value: &Value,
    ) -> Result<()> {
        let id = self.element_of(element)?;
        let value = js::to_string(value);
        let mut tree = self.tree();
        let element = tree.element_mut(id).expect("an element");
        match namespace {
            None => {
                NameKind::Attribute.validate(name)?;
                let name = match element.is_html() {
                    true => name.to_ascii_lowercase(),
                    false => name.to_owned(),
                };
                let existing = element
                    .attrs
                    .iter_mut()
                    .find(|attr| crate::tree::qualified(&attr.name) == name);
                match existing {
                    Some(attr) => attr.value = value,
                    None => element.attrs.push(Attr {
                        name: QualName::new(None, ns!(), LocalName::from(name)),
                        value,
                    }),
                }
            }
            Some(namespace) => {
                let name = names::validate_and_extract(namespace, name, NameKind::Attribute)?;
                element.set_attr_ns(qual_name(name), value);
            }
        }
        element.style = None;
        Ok(())
    }

    fn set_style(&self, element: &HtmlNode, css: &Value) -> Result<bool> {
        let id = self.element_of(element)?;
        let mut tree = self.tree();
        let element = tree.element_mut(id).expect("an element");
        if element.space() == Space::Other {
            return Ok(false);
        }
        // The attribute gets the declarations kept, not the text given.
        let style = Declarations::parse(&js::to_string(css));
        let name = QualName::new(None, ns!(), local_name!("style"));
        element.set_attr_ns(name, style.css_text());
        element.style = Some(style);
        Ok(true)
    }

    fn stringify(&self, node: &HtmlNode) -> Result<String> {
        let tree = node.dom.tree();
        Ok(match &tree.node(node.id).data {
            // A link stringifies to its `href`, which jsdom resolves against the document's URL
            // and this keeps as written.
            Data::Element(element)
                if element.is_html()
                    && matches!(element.name.local, local_name!("a") | local_name!("area")) =>
            {
                element.attribute("href").unwrap_or_default().to_owned()
            }
            Data::Element(element) => format!("[object {}]", interface(element)),
            Data::Text(_) => "[object Text]".into(),
            Data::Comment(_) => "[object Comment]".into(),
            Data::Document => "[object Document]".into(),
            Data::Fragment => "[object DocumentFragment]".into(),
            Data::Doctype { .. } => "[object DocumentType]".into(),
            Data::ProcessingInstruction { .. } => "[object ProcessingInstruction]".into(),
        })
    }
}
