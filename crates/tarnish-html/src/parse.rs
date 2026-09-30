//! Parsing HTML into the arena, as the linkedom fork parses it with parse5: html5gum tokenizes,
//! and html5ever's tree builder builds from its tokens. html5ever's own tokenizer takes twice as
//! long. A byte order mark stays as text.

use std::borrow::Cow;
use std::cell::{Cell, Ref, RefCell};

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::states::RawKind;
use html5ever::tokenizer::{Doctype, Tag, TagKind, Token, TokenSink, TokenSinkResult};
use html5ever::tree_builder::{
    ElementFlags, NodeOrText, QuirksMode, TreeBuilder, TreeBuilderOpts, TreeSink, create_element,
};
use html5ever::{Attribute, LocalName, QualName, local_name, ns};
use html5gum::{Emitter, Error, State, Tokenizer};
use rustc_hash::FxHashMap;
use tarnish::js::deadline::Deadline;

use crate::tree::{Attr, Data, Element, NodeId, Tree};

/// Without scripting, as a document with no browsing context parses: a `<noscript>` holds
/// elements, not text.
fn tree_builder_options() -> TreeBuilderOpts {
    TreeBuilderOpts {
        scripting_enabled: false,
        ..TreeBuilderOpts::default()
    }
}

pub(crate) fn document(tree: &mut Tree, html: &str) -> NodeId {
    let document = tree.push(Data::Document);
    let builder = TreeBuilder::new(Sink::new(tree, document), tree_builder_options());
    let quirks = run(builder, html).quirks.get();
    tree.quirks = quirks;
    document
}

/// Parse a fragment as a `<template>`'s content holds it, into a new document fragment. A
/// template's content starts in the data state, as a document does.
pub(crate) fn fragment(tree: &mut Tree, html: &str) -> NodeId {
    let document = tree.push(Data::Document);
    let sink = Sink::new(tree, document);
    let context = QualName::new(None, ns!(html), local_name!("template"));
    let context = create_element(&sink, context, Vec::new());
    let builder = TreeBuilder::new_for_fragment(sink, context, None, tree_builder_options());
    run(builder, html);
    let fragment = tree.push(Data::Fragment);
    // The fragment's nodes are the children of the one element the parser made the root.
    if let Some(root) = tree.node(document).first {
        tree.reparent_children(root, fragment);
    }
    fragment
}

/// Tokenizes the HTML into the tree builder. Past the deadline of what runs on this thread, the
/// tokenizer stops, and what the caller makes of the tree goes unused.
fn run<'t>(builder: TreeBuilder<NodeId, Sink<'t>>, html: &str) -> Sink<'t> {
    let mut feed = Feed::new(builder);
    let _ = Tokenizer::new_with_emitter(html, &mut feed).next();
    if !feed.late {
        feed.builder.end();
    }
    NAMES.set(feed.names.0);
    feed.builder.sink
}

/// Hands html5gum's tokens to html5ever's tree builder, and switches the tokenizer's state as the
/// tree builder asks.
struct Feed<'t> {
    builder: TreeBuilder<NodeId, Sink<'t>>,
    /// Each tag is a turn of the parse, which can take the tree builder through every element
    /// it has open and every active formatting element: the sink counts them.
    deadline: Deadline,
    /// Whether the deadline has passed, which stops the tokenizer.
    late: bool,
    /// Text read since the last token that isn't text.
    text: Vec<u8>,
    tag: Vec<u8>,
    is_end_tag: bool,
    self_closing: bool,
    attrs: Vec<Attribute>,
    /// Whether an attribute is being read that the tag keeps.
    in_attribute: bool,
    attribute_name: Vec<u8>,
    attribute_value: Vec<u8>,
    last_start_tag: Vec<u8>,
    comment: Vec<u8>,
    doctype: DoctypeBytes,
    names: Names,
}

#[derive(Default)]
struct DoctypeBytes {
    name: Option<Vec<u8>>,
    public_id: Option<Vec<u8>>,
    system_id: Option<Vec<u8>>,
    force_quirks: bool,
}

impl<'t> Feed<'t> {
    fn new(builder: TreeBuilder<NodeId, Sink<'t>>) -> Self {
        Feed {
            builder,
            deadline: Deadline::current(),
            late: false,
            text: Vec::new(),
            tag: Vec::new(),
            is_end_tag: false,
            self_closing: false,
            attrs: Vec::new(),
            in_attribute: false,
            attribute_name: Vec::new(),
            attribute_value: Vec::new(),
            last_start_tag: Vec::new(),
            comment: Vec::new(),
            doctype: DoctypeBytes::default(),
            names: Names(NAMES.take()),
        }
    }

    fn process(&self, token: Token) -> TokenSinkResult<NodeId> {
        self.builder.process_token(token, 1)
    }

    /// Hands on a token that isn't a tag: the tree builder switches the tokenizer's state only
    /// in answer to a tag.
    fn send(&self, token: Token) {
        let _ = self.process(token);
    }

    /// Hands on the text read, with each U+0000 as a token of its own, as html5ever's tokenizer
    /// emits it.
    fn flush_text(&mut self) {
        let mut rest = &self.text[..];
        while let Some(nul) = memchr::memchr(0, rest) {
            if nul > 0 {
                self.send(Token::CharacterTokens(tendril(&rest[..nul])));
            }
            self.send(Token::NullCharacterToken);
            rest = &rest[nul + 1..];
        }
        if !rest.is_empty() {
            self.send(Token::CharacterTokens(tendril(rest)));
        }
        self.text.clear();
    }

    fn end_attribute(&mut self) {
        if !std::mem::take(&mut self.in_attribute) {
            return;
        }
        let name = self.names.atom(&self.attribute_name);
        if self.attrs.iter().all(|attr| attr.name.local != name) {
            self.attrs.push(Attribute {
                name: QualName::new(None, ns!(), name),
                value: tendril(&self.attribute_value),
            });
        }
    }
}

thread_local! {
    /// The atoms of the tag and attribute names parses on this thread have read, which each parse
    /// takes and gives back. A name that isn't one of html5ever's own, as a `data-*` attribute's
    /// isn't, takes a lock to make and another to drop, and these don't drop.
    static NAMES: Cell<FxHashMap<Box<[u8]>, LocalName>> = Cell::default();
}

/// A thread keeps up to `NAMES_KEPT` names of up to `LONGEST_NAME` bytes, as HTML from elsewhere
/// can hold any number of names of any length.
const NAMES_KEPT: usize = 1024;
const LONGEST_NAME: usize = 64;

struct Names(FxHashMap<Box<[u8]>, LocalName>);

impl Names {
    fn atom(&mut self, name: &[u8]) -> LocalName {
        if name.len() > LONGEST_NAME {
            return LocalName::from(&*text(name));
        }
        if let Some(atom) = self.0.get(name) {
            return atom.clone();
        }
        if self.0.len() >= NAMES_KEPT {
            self.0.clear();
        }
        let atom = LocalName::from(&*text(name));
        self.0.insert(name.into(), atom.clone());
        atom
    }
}

/// What html5gum writes for a token, which is UTF-8 as it reads a `&str`.
fn text(bytes: &[u8]) -> Cow<'_, str> {
    match std::str::from_utf8(bytes) {
        Ok(text) => Cow::Borrowed(text),
        Err(_) => String::from_utf8_lossy(bytes),
    }
}

fn tendril(bytes: &[u8]) -> StrTendril {
    StrTendril::from_slice(&text(bytes))
}

impl Emitter for &mut Feed<'_> {
    type Token = ();

    fn set_last_start_tag(&mut self, last_start_tag: Option<&[u8]>) {
        self.last_start_tag.clear();
        self.last_start_tag
            .extend_from_slice(last_start_tag.unwrap_or_default());
    }

    fn emit_eof(&mut self) {
        self.flush_text();
        self.send(Token::EOFToken);
    }

    fn emit_error(&mut self, _: Error) {}

    fn should_emit_errors(&mut self) -> bool {
        false
    }

    fn pop_token(&mut self) -> Option<()> {
        self.late.then_some(())
    }

    fn emit_string(&mut self, string: &[u8]) {
        self.text.extend_from_slice(string);
    }

    fn init_start_tag(&mut self) {
        self.tag.clear();
        self.is_end_tag = false;
        self.self_closing = false;
        self.attrs.clear();
        self.in_attribute = false;
    }

    fn init_end_tag(&mut self) {
        self.init_start_tag();
        self.is_end_tag = true;
    }

    fn init_comment(&mut self) {
        self.comment.clear();
    }

    fn emit_current_tag(&mut self) -> Option<State> {
        self.end_attribute();
        self.flush_text();
        let name = self.names.atom(&self.tag);
        let kind = match self.is_end_tag {
            true => TagKind::EndTag,
            false => {
                self.last_start_tag.clear();
                self.last_start_tag.extend_from_slice(&self.tag);
                TagKind::StartTag
            }
        };
        let tag = Tag {
            kind,
            name,
            self_closing: self.self_closing,
            attrs: std::mem::take(&mut self.attrs),
            had_duplicate_attributes: false,
        };
        let state = match self.process(Token::TagToken(tag)) {
            // A `<meta charset>` names an encoding, which text that is already decoded ignores.
            TokenSinkResult::Continue | TokenSinkResult::EncodingIndicator(_) => None,
            TokenSinkResult::Script(_) => Some(State::Data),
            TokenSinkResult::Plaintext => Some(State::PlainText),
            TokenSinkResult::RawData(RawKind::Rcdata) => Some(State::RcData),
            TokenSinkResult::RawData(RawKind::Rawtext) => Some(State::RawText),
            TokenSinkResult::RawData(RawKind::ScriptData | RawKind::ScriptDataEscaped(_)) => {
                Some(State::ScriptData)
            }
        };
        let scanned = self.builder.sink.scanned.take();
        self.late = self.deadline.turn(scanned).is_err();
        state
    }

    fn emit_current_comment(&mut self) {
        self.flush_text();
        let comment = tendril(&self.comment);
        self.send(Token::CommentToken(comment));
    }

    fn emit_current_doctype(&mut self) {
        self.flush_text();
        let DoctypeBytes {
            name,
            public_id,
            system_id,
            force_quirks,
        } = std::mem::take(&mut self.doctype);
        let decode = |bytes: Option<Vec<u8>>| bytes.map(|bytes| tendril(&bytes));
        self.send(Token::DoctypeToken(Doctype {
            name: decode(name),
            public_id: decode(public_id),
            system_id: decode(system_id),
            force_quirks,
        }));
    }

    fn set_self_closing(&mut self) {
        self.self_closing = true;
    }

    fn set_force_quirks(&mut self) {
        self.doctype.force_quirks = true;
    }

    fn push_tag_name(&mut self, name: &[u8]) {
        self.tag.extend_from_slice(name);
    }

    fn push_comment(&mut self, comment: &[u8]) {
        self.comment.extend_from_slice(comment);
    }

    fn push_doctype_name(&mut self, name: &[u8]) {
        self.doctype
            .name
            .get_or_insert_default()
            .extend_from_slice(name);
    }

    fn init_doctype(&mut self) {
        self.doctype = DoctypeBytes::default();
    }

    fn init_attribute(&mut self) {
        self.end_attribute();
        if !self.is_end_tag {
            self.in_attribute = true;
            self.attribute_name.clear();
            self.attribute_value.clear();
        }
    }

    fn push_attribute_name(&mut self, name: &[u8]) {
        if self.in_attribute {
            self.attribute_name.extend_from_slice(name);
        }
    }

    fn push_attribute_value(&mut self, value: &[u8]) {
        if self.in_attribute {
            self.attribute_value.extend_from_slice(value);
        }
    }

    fn set_doctype_public_identifier(&mut self, value: &[u8]) {
        self.doctype.public_id = Some(value.to_vec());
    }

    fn set_doctype_system_identifier(&mut self, value: &[u8]) {
        self.doctype.system_id = Some(value.to_vec());
    }

    fn push_doctype_public_identifier(&mut self, value: &[u8]) {
        if let Some(id) = &mut self.doctype.public_id {
            id.extend_from_slice(value);
        }
    }

    fn push_doctype_system_identifier(&mut self, value: &[u8]) {
        if let Some(id) = &mut self.doctype.system_id {
            id.extend_from_slice(value);
        }
    }

    fn current_is_appropriate_end_tag_token(&mut self) -> bool {
        self.is_end_tag && !self.last_start_tag.is_empty() && self.tag == self.last_start_tag
    }

    fn adjusted_current_node_present_but_not_in_html_namespace(&mut self) -> bool {
        // Text still held here can change the current node, as it reopens formatting elements.
        self.flush_text();
        self.builder
            .adjusted_current_node_present_but_not_in_html_namespace()
    }
}

struct Sink<'t> {
    tree: RefCell<&'t mut Tree>,
    document: NodeId,
    quirks: Cell<bool>,
    /// The elements the tree builder has looked at or made since the last tag.
    scanned: Cell<usize>,
}

impl<'t> Sink<'t> {
    fn new(tree: &'t mut Tree, document: NodeId) -> Self {
        Sink {
            tree: RefCell::new(tree),
            document,
            quirks: Cell::new(false),
            scanned: Cell::new(0),
        }
    }

    fn scan(&self) {
        self.scanned.set(self.scanned.get() + 1);
    }
}

/// The attributes, with the empty prefix html5ever gives `xmlns` as none, as the standard's
/// "adjust foreign attributes" does.
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
            value: attr.value,
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
        self.scan();
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
        self.scan();
        let mut tree = self.tree.borrow_mut();
        let template_contents = flags.template.then(|| tree.push(Data::Fragment));
        let mut element = Element::new(name, attrs(attributes));
        element.template_contents = template_contents;
        element.integration_point = flags.mathml_annotation_xml_integration_point;
        tree.push(Data::Element(Box::new(element)))
    }

    fn create_comment(&self, text: StrTendril) -> NodeId {
        self.tree
            .borrow_mut()
            .push(Data::Comment(String::from(&*text)))
    }

    fn create_pi(&self, _: StrTendril, _: StrTendril) -> NodeId {
        unreachable!("HTML parses a processing instruction as a comment")
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
            name: String::from(&*name),
        });
        tree.append(self.document, doctype);
    }

    fn get_template_contents(&self, target: &NodeId) -> NodeId {
        let tree = self.tree.borrow();
        let element = tree.element(*target).expect("a template");
        element.template_contents.expect("a template's content")
    }

    fn same_node(&self, x: &NodeId, y: &NodeId) -> bool {
        self.scan();
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
            if element.attr_ns(&attr.name.ns, &attr.name.local).is_none() {
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

    // DOMParser and innerHTML never make declarative shadow roots; markup5ever's default does.
    fn allow_declarative_shadow_roots(&self, _: &NodeId) -> bool {
        false
    }
}
