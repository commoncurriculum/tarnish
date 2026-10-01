//! The arena a DOM's nodes live in, linked to each other by index.

use std::sync::Arc;

use html5ever::tendril::StrTendril;
use html5ever::{LocalName, Namespace, QualName, ns};
use tarnish_css::Declarations;

pub(crate) type NodeId = usize;

#[derive(Default)]
pub(crate) struct Tree {
    nodes: Vec<Node>,
    /// Whether the document parsed into the tree is in quirks mode, where class and id
    /// selectors ignore case.
    pub(crate) quirks: bool,
}

pub(crate) struct Node {
    pub(crate) parent: Option<NodeId>,
    pub(crate) previous: Option<NodeId>,
    pub(crate) next: Option<NodeId>,
    pub(crate) first: Option<NodeId>,
    pub(crate) last: Option<NodeId>,
    pub(crate) data: Data,
}

pub(crate) enum Data {
    Document,
    Fragment,
    Doctype { name: String },
    Text(String),
    Comment(String),
    Element(Box<Element>),
}

pub(crate) struct Element {
    pub(crate) name: QualName,
    pub(crate) attrs: Vec<Attr>,
    /// A `<template>`'s content, a fragment that isn't among its children.
    pub(crate) template_contents: Option<NodeId>,
    pub(crate) integration_point: bool,
    /// The declarations of the `style` attribute, read when first asked for.
    pub(crate) style: Option<Arc<Declarations>>,
}

pub(crate) struct Attr {
    pub(crate) name: QualName,
    pub(crate) value: StrTendril,
}

/// The namespaces whose elements have `style` and are written by their local name.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Space {
    Html,
    Svg,
    MathMl,
    Other,
}

impl Element {
    pub(crate) fn new(name: QualName, attrs: Vec<Attr>) -> Element {
        Element {
            name,
            attrs,
            template_contents: None,
            integration_point: false,
            style: None,
        }
    }

    pub(crate) fn space(&self) -> Space {
        match self.name.ns {
            ns!(html) => Space::Html,
            ns!(svg) => Space::Svg,
            ns!(mathml) => Space::MathMl,
            _ => Space::Other,
        }
    }

    pub(crate) fn is_html(&self) -> bool {
        self.name.ns == ns!(html)
    }

    pub(crate) fn attr_ns(&self, namespace: &Namespace, local: &LocalName) -> Option<&Attr> {
        let mut attrs = self.attrs.iter();
        attrs.find(|attr| attr.name.ns == *namespace && attr.name.local == *local)
    }

    /// Set the value of the attribute of the name's namespace and local name, or add one, as
    /// `setAttributeNS` does.
    pub(crate) fn set_attr_ns(&mut self, name: QualName, value: String) {
        let mut attrs = self.attrs.iter_mut();
        match attrs.find(|attr| attr.name.ns == name.ns && attr.name.local == name.local) {
            Some(attr) => attr.value = value.into(),
            None => self.attrs.push(Attr {
                name,
                value: value.into(),
            }),
        }
    }

    /// `qualifiedName`: the prefix and the local name.
    pub(crate) fn qualified_name(&self) -> String {
        qualified(&self.name)
    }

    /// `getAttribute`: the first attribute of this qualified name, lower-cased on an HTML
    /// element.
    pub(crate) fn attribute(&self, name: &str) -> Option<&str> {
        let lower = self.is_html();
        let attr = self
            .attrs
            .iter()
            .find(|attr| is_qualified(&attr.name, name, lower));
        attr.map(|attr| &*attr.value)
    }
}

pub(crate) fn qualified(name: &QualName) -> String {
    match &name.prefix {
        Some(prefix) => format!("{prefix}:{}", name.local),
        None => String::from(&*name.local),
    }
}

/// Whether `name`, in lower case when `lower`, is `qualified`'s qualified name.
pub(crate) fn is_qualified(qualified: &QualName, name: &str, lower: bool) -> bool {
    let same = |part: &[u8], of: &[u8]| {
        part.len() == of.len()
            && part.iter().zip(of).all(|(&part, &of)| match lower {
                true => part == of.to_ascii_lowercase(),
                false => part == of,
            })
    };
    let (name, local) = (name.as_bytes(), qualified.local.as_bytes());
    match &qualified.prefix {
        None => same(local, name),
        Some(prefix) => {
            let prefix = prefix.as_bytes();
            name.len() == prefix.len() + 1 + local.len()
                && same(prefix, &name[..prefix.len()])
                && name[prefix.len()] == b':'
                && same(local, &name[prefix.len() + 1..])
        }
    }
}

pub(crate) const PRECEDING: u16 = 0x02;
pub(crate) const FOLLOWING: u16 = 0x04;
const DISCONNECTED: u16 = 0x01;
const CONTAINS: u16 = 0x08;
const CONTAINED_BY: u16 = 0x10;
const IMPLEMENTATION_SPECIFIC: u16 = 0x20;

impl Tree {
    pub(crate) fn push(&mut self, data: Data) -> NodeId {
        self.nodes.push(Node {
            parent: None,
            previous: None,
            next: None,
            first: None,
            last: None,
            data,
        });
        self.nodes.len() - 1
    }

    /// How many nodes it has ever held, which no path down it is longer than.
    pub(crate) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub(crate) fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    pub(crate) fn element(&self, id: NodeId) -> Option<&Element> {
        match &self.nodes[id].data {
            Data::Element(element) => Some(element),
            _ => None,
        }
    }

    pub(crate) fn element_mut(&mut self, id: NodeId) -> Option<&mut Element> {
        match &mut self.nodes[id].data {
            Data::Element(element) => Some(element),
            _ => None,
        }
    }

    pub(crate) fn children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(self.nodes[id].first, |&child| self.nodes[child].next)
    }

    pub(crate) fn ancestors(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(self.nodes[id].parent, |&node| self.nodes[node].parent)
    }

    pub(crate) fn root(&self, id: NodeId) -> NodeId {
        self.ancestors(id).last().unwrap_or(id)
    }

    /// Whether `node` is `ancestor` or inside it.
    pub(crate) fn contains(&self, ancestor: NodeId, node: NodeId) -> bool {
        node == ancestor || self.ancestors(node).any(|above| above == ancestor)
    }

    pub(crate) fn detach(&mut self, id: NodeId) {
        let Node {
            parent,
            previous,
            next,
            ..
        } = self.nodes[id];
        let Some(parent) = parent else { return };
        match previous {
            Some(previous) => self.nodes[previous].next = next,
            None => self.nodes[parent].first = next,
        }
        match next {
            Some(next) => self.nodes[next].previous = previous,
            None => self.nodes[parent].last = previous,
        }
        let node = &mut self.nodes[id];
        (node.parent, node.previous, node.next) = (None, None, None);
    }

    /// Move `child` to the end of `parent`'s children.
    pub(crate) fn append(&mut self, parent: NodeId, child: NodeId) {
        self.detach(child);
        let last = self.nodes[parent].last;
        match last {
            Some(last) => self.nodes[last].next = Some(child),
            None => self.nodes[parent].first = Some(child),
        }
        let node = &mut self.nodes[child];
        (node.parent, node.previous) = (Some(parent), last);
        self.nodes[parent].last = Some(child);
    }

    /// Move `child` to just before `sibling`, which has a parent.
    pub(crate) fn insert_before(&mut self, sibling: NodeId, child: NodeId) {
        self.detach(child);
        let Some(parent) = self.nodes[sibling].parent else {
            return;
        };
        let previous = self.nodes[sibling].previous;
        match previous {
            Some(previous) => self.nodes[previous].next = Some(child),
            None => self.nodes[parent].first = Some(child),
        }
        self.nodes[sibling].previous = Some(child);
        let node = &mut self.nodes[child];
        (node.parent, node.previous, node.next) = (Some(parent), previous, Some(sibling));
    }

    /// Move every child of `from` to the end of `to`'s children.
    pub(crate) fn reparent_children(&mut self, from: NodeId, to: NodeId) {
        while let Some(child) = self.nodes[from].first {
            self.append(to, child);
        }
    }

    /// Add text to the end of `parent`, into the text node that ends it if there is one.
    pub(crate) fn append_text(&mut self, parent: NodeId, text: &str) {
        if let Some(last) = self.nodes[parent].last
            && let Data::Text(existing) = &mut self.nodes[last].data
        {
            existing.push_str(text);
            return;
        }
        let node = self.push(Data::Text(text.to_owned()));
        self.append(parent, node);
    }

    /// Add text just before `sibling`, into the text node before it if there is one.
    pub(crate) fn insert_text_before(&mut self, sibling: NodeId, text: &str) {
        if let Some(previous) = self.nodes[sibling].previous
            && let Data::Text(existing) = &mut self.nodes[previous].data
        {
            existing.push_str(text);
            return;
        }
        let node = self.push(Data::Text(text.to_owned()));
        self.insert_before(sibling, node);
    }

    /// `a.compareDocumentPosition(b)`.
    pub(crate) fn compare_document_position(&self, a: NodeId, b: NodeId) -> u16 {
        if a == b {
            return 0;
        }
        let (root_a, root_b) = (self.root(a), self.root(b));
        if root_a != root_b {
            let order = if root_b < root_a {
                PRECEDING
            } else {
                FOLLOWING
            };
            return DISCONNECTED | IMPLEMENTATION_SPECIFIC | order;
        }
        if self.contains(b, a) {
            return CONTAINS | PRECEDING;
        }
        if self.contains(a, b) {
            return CONTAINED_BY | FOLLOWING;
        }
        let path = |node: NodeId| {
            let mut path: Vec<NodeId> = std::iter::once(node).chain(self.ancestors(node)).collect();
            path.reverse();
            path
        };
        let (path_a, path_b) = (path(a), path(b));
        let shared = path_a
            .iter()
            .zip(&path_b)
            .take_while(|(a, b)| a == b)
            .count();
        let (branch_a, branch_b) = (path_a[shared], path_b[shared]);
        let mut sibling = self.nodes[branch_a].next;
        while let Some(next) = sibling {
            if next == branch_b {
                return FOLLOWING;
            }
            sibling = self.nodes[next].next;
        }
        PRECEDING
    }
}
