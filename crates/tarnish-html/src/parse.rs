//! Parsing HTML into the arena with html5ever, as the linkedom fork parses it with parse5,
//! keeping a byte order mark as text.

use std::borrow::Cow;
use std::cell::{Cell, Ref, RefCell};

use html5ever::driver::{self, ParseOpts};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tokenizer::TokenizerOpts;
use html5ever::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeBuilderOpts, TreeSink};
use html5ever::{Attribute, QualName, local_name, ns};

use crate::tree::{Attr, Data, Element, NodeId, Tree};

/// Without scripting, as a document with no browsing context parses: a `<noscript>` holds
/// elements, not text.
fn options() -> ParseOpts {
    ParseOpts {
        tokenizer: TokenizerOpts {
            discard_bom: false,
            ..TokenizerOpts::default()
        },
        tree_builder: TreeBuilderOpts {
            scripting_enabled: false,
            ..TreeBuilderOpts::default()
        },
    }
}

/// Parse a document into a new document node.
pub(crate) fn document(tree: &mut Tree, html: &str) -> NodeId {
    let document = tree.push(Data::Document);
    let sink = Sink::new(tree, document);
    tree.quirks = driver::parse_document(sink, options()).one(html);
    document
}

/// Parse a fragment as a `<template>`'s content holds it, into a new document fragment.
pub(crate) fn fragment(tree: &mut Tree, html: &str) -> NodeId {
    let document = tree.push(Data::Document);
    let context = QualName::new(None, ns!(html), local_name!("template"));
    let sink = Sink::new(tree, document);
    driver::parse_fragment(sink, options(), context, Vec::new(), false).one(html);
    let fragment = tree.push(Data::Fragment);
    // The fragment's nodes are the children of the one element the parser made the root.
    if let Some(root) = tree.node(document).first {
        tree.reparent_children(root, fragment);
    }
    fragment
}

struct Sink<'t> {
    tree: RefCell<&'t mut Tree>,
    document: NodeId,
    quirks: Cell<bool>,
}

impl<'t> Sink<'t> {
    fn new(tree: &'t mut Tree, document: NodeId) -> Self {
        Sink {
            tree: RefCell::new(tree),
            document,
            quirks: Cell::new(false),
        }
    }
}

/// The attributes, with the empty prefix html5ever gives `xmlns` as none, as jsdom does.
fn attrs(attrs: Vec<Attribute>) -> Vec<Attr> {
    let attrs = attrs.into_iter().map(|mut attr| {
        if attr
            .name
            .prefix
            .as_ref()
            .is_some_and(|prefix| prefix.is_empty())
        {
            attr.name.prefix = None;
        }
        Attr {
            name: attr.name,
            value: attr.value.to_string(),
        }
    });
    attrs.collect()
}

impl TreeSink for Sink<'_> {
    type Handle = NodeId;
    /// Whether the document is in quirks mode.
    type Output = bool;
    type ElemName<'a>
        = Ref<'a, QualName>
    where
        Self: 'a;

    fn finish(self) -> bool {
        self.quirks.get()
    }

    fn parse_error(&self, _: Cow<'static, str>) {}

    fn get_document(&self) -> NodeId {
        self.document
    }

    fn elem_name<'a>(&'a self, target: &'a NodeId) -> Ref<'a, QualName> {
        Ref::map(self.tree.borrow(), |tree| {
            &tree.element(*target).expect("an element").name
        })
    }

    fn create_element(
        &self,
        name: QualName,
        attributes: Vec<Attribute>,
        flags: ElementFlags,
    ) -> NodeId {
        let mut tree = self.tree.borrow_mut();
        let template_contents = flags.template.then(|| tree.push(Data::Fragment));
        let mut element = Element::new(name, attrs(attributes));
        element.template_contents = template_contents;
        element.integration_point = flags.mathml_annotation_xml_integration_point;
        tree.push(Data::Element(Box::new(element)))
    }

    fn create_comment(&self, text: StrTendril) -> NodeId {
        self.tree.borrow_mut().push(Data::Comment(text.to_string()))
    }

    fn create_pi(&self, target: StrTendril, data: StrTendril) -> NodeId {
        self.tree.borrow_mut().push(Data::ProcessingInstruction {
            target: target.to_string(),
            data: data.to_string(),
        })
    }

    fn append(&self, parent: &NodeId, child: NodeOrText<NodeId>) {
        let mut tree = self.tree.borrow_mut();
        match child {
            NodeOrText::AppendNode(node) => tree.append(*parent, node),
            NodeOrText::AppendText(text) => tree.append_text(*parent, &text),
        }
    }

    fn append_based_on_parent_node(
        &self,
        element: &NodeId,
        prev_element: &NodeId,
        child: NodeOrText<NodeId>,
    ) {
        let has_parent = self.tree.borrow().node(*element).parent.is_some();
        match has_parent {
            true => self.append_before_sibling(element, child),
            false => self.append(prev_element, child),
        }
    }

    fn append_doctype_to_document(&self, name: StrTendril, _: StrTendril, _: StrTendril) {
        let mut tree = self.tree.borrow_mut();
        let doctype = tree.push(Data::Doctype {
            name: name.to_string(),
        });
        tree.append(self.document, doctype);
    }

    fn get_template_contents(&self, target: &NodeId) -> NodeId {
        let tree = self.tree.borrow();
        let element = tree.element(*target).expect("a template");
        element.template_contents.expect("a template's content")
    }

    fn same_node(&self, x: &NodeId, y: &NodeId) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, mode: QuirksMode) {
        self.quirks.set(mode == QuirksMode::Quirks);
    }

    fn append_before_sibling(&self, sibling: &NodeId, new_node: NodeOrText<NodeId>) {
        let mut tree = self.tree.borrow_mut();
        match new_node {
            NodeOrText::AppendNode(node) => tree.insert_before(*sibling, node),
            NodeOrText::AppendText(text) => tree.insert_text_before(*sibling, &text),
        }
    }

    fn add_attrs_if_missing(&self, target: &NodeId, attributes: Vec<Attribute>) {
        let mut tree = self.tree.borrow_mut();
        let element = tree.element_mut(*target).expect("an element");
        for attr in attrs(attributes) {
            let exists = element.attrs.iter().any(|existing| {
                existing.name.ns == attr.name.ns && existing.name.local == attr.name.local
            });
            if !exists {
                element.attrs.push(attr);
            }
        }
        element.style = None;
    }

    fn remove_from_parent(&self, target: &NodeId) {
        self.tree.borrow_mut().detach(*target);
    }

    fn reparent_children(&self, node: &NodeId, new_parent: &NodeId) {
        self.tree.borrow_mut().reparent_children(*node, *new_parent);
    }

    fn is_mathml_annotation_xml_integration_point(&self, handle: &NodeId) -> bool {
        let tree = self.tree.borrow();
        tree.element(*handle)
            .is_some_and(|element| element.integration_point)
    }

    // parse5 7, which jsdom 20 parses with, has no declarative shadow roots.
    fn allow_declarative_shadow_roots(&self, _: &NodeId) -> bool {
        false
    }
}
