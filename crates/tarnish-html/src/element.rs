//! The DOM's own API on its nodes, as a parse rule's hooks call it on the element they're given:
//! reading an element's tag, text and relatives, finding elements by selector, and the few
//! changes a hook makes to the DOM it parses.

use html5ever::ns;
use tarnish::dom::{Dom, NodeKind};
use tarnish::{Result, Value};

use crate::dom::HtmlNode;
use crate::select;
use crate::tree::{Data, NodeId};

impl HtmlNode {
    /// What kind of node it is.
    pub fn kind(&self) -> NodeKind {
        match self.dom.tree().node(self.id).data {
            Data::Element(_) => NodeKind::Element,
            Data::Text(_) => NodeKind::Text,
            _ => NodeKind::Other,
        }
    }

    /// `nodeName === tag`, for an element: an HTML element's name is compared without regard to
    /// case, as its `nodeName` is its name in upper case.
    pub fn is_tag(&self, tag: &str) -> bool {
        self.dom
            .tree()
            .element(self.id)
            .is_some_and(|element| match element.name.ns == ns!(html) {
                true => (*element.name.local).eq_ignore_ascii_case(tag),
                false => element.name.prefix.is_none() && &*element.name.local == tag,
            })
    }

    /// `localName`, for an element.
    pub fn local_name(&self) -> Option<String> {
        let tree = self.dom.tree();
        Some(String::from(&*tree.element(self.id)?.name.local))
    }

    /// `textContent`: an element's text, a text node's own.
    pub fn text_content(&self) -> String {
        fn push(tree: &crate::tree::Tree, id: NodeId, text: &mut String) {
            match &tree.node(id).data {
                Data::Text(own) => text.push_str(own),
                Data::Element(_) | Data::Fragment | Data::Document => {
                    for child in tree.children(id) {
                        tarnish::js::stack::grow(|| push(tree, child, text));
                    }
                }
                _ => {}
            }
        }
        let tree = self.dom.tree();
        let mut text = String::new();
        push(&tree, self.id, &mut text);
        text
    }

    pub fn parent_element(&self) -> Option<HtmlNode> {
        let tree = self.dom.tree();
        let parent = tree.node(self.id).parent?;
        tree.element(parent)?;
        drop(tree);
        Some(self.dom.node(parent))
    }

    pub fn first_child(&self) -> Option<HtmlNode> {
        let first = self.dom.tree().node(self.id).first?;
        Some(self.dom.node(first))
    }

    /// `childNodes`.
    pub fn child_nodes(&self) -> Vec<HtmlNode> {
        let children: Vec<NodeId> = self.dom.tree().children(self.id).collect();
        children.into_iter().map(|id| self.dom.node(id)).collect()
    }

    /// `children`: the child elements.
    pub fn children(&self) -> Vec<HtmlNode> {
        let tree = self.dom.tree();
        let children: Vec<NodeId> = tree
            .children(self.id)
            .filter(|&child| tree.element(child).is_some())
            .collect();
        drop(tree);
        children.into_iter().map(|id| self.dom.node(id)).collect()
    }

    /// `querySelector`: the first element inside this one that matches the selector.
    pub fn query_selector(&self, selector: &str) -> Result<Option<HtmlNode>> {
        self.dom.query_selector(self, selector)
    }

    /// `closest`: this element or the nearest one above it that matches the selector.
    pub fn closest(&self, selector: &str) -> Result<Option<HtmlNode>> {
        let selectors = self.dom.selectors(selector)?;
        let found = select::closest(&self.dom.tree(), self.id, &selectors);
        Ok(found.map(|id| self.dom.node(id)))
    }

    /// `setAttribute(name, value)` on an HTML element.
    pub fn set_attribute(&self, name: &str, value: &str) -> Result<()> {
        self.dom
            .set_attribute(self, None, name, &Value::String(value.into()))
    }

    /// `append(node)`.
    pub fn append_child(&self, node: &HtmlNode) -> Result<()> {
        self.dom.append_child(self, node)
    }

    /// `prepend(node)`: `node` before the first child.
    pub fn prepend(&self, node: &HtmlNode) -> Result<()> {
        match self.first_child() {
            Some(first) => first.before(node),
            None => self.append_child(node),
        }
    }

    /// `before(node)`: `node` just before this one, which has a parent.
    pub fn before(&self, node: &HtmlNode) -> Result<()> {
        self.dom.own_id(node)?;
        self.dom.tree().insert_before(self.id, node.id);
        Ok(())
    }

    /// `remove()`.
    pub fn remove(&self) {
        self.dom.tree().detach(self.id);
    }
}

impl crate::dom::HtmlDom {
    /// `createElement(tag)`, of an HTML element.
    pub fn create_html_element(&self, tag: &str) -> Result<HtmlNode> {
        self.create_element(None, tag)
    }

    /// `createTextNode(text)`.
    pub fn create_text_node(&self, text: &str) -> HtmlNode {
        let id = self.tree().push(Data::Text(text.to_owned()));
        self.node(id)
    }
}
