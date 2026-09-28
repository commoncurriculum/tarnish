//! Matching CSS selectors, with servo's `selectors`.

use std::fmt;

use cssparser::ToCss;
use html5ever::{LocalName, Namespace, local_name, ns};
use precomputed_hash::PrecomputedHash;
use selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use selectors::bloom::BloomFilter;
use selectors::matching::{
    self, ElementSelectorFlags, MatchingContext, MatchingForInvalidation, MatchingMode,
    NeedsSelectorFlags, QuirksMode, SelectorCaches,
};
use selectors::parser::{self, ParseRelative, SelectorList, SelectorParseErrorKind};
use selectors::{Element, OpaqueElement};

use crate::tree::{Data, NodeId, Tree};

pub(crate) type Selectors = SelectorList<Impl>;

pub(crate) fn parse(selector: &str) -> Option<Selectors> {
    let mut input = cssparser::ParserInput::new(selector);
    let mut input = cssparser::Parser::new(&mut input);
    SelectorList::parse(&SelectorParser, &mut input, ParseRelative::No).ok()
}

/// Whether the element matches, with `scope` as `:scope`.
pub(crate) fn matches(tree: &Tree, element: NodeId, selectors: &Selectors, scope: NodeId) -> bool {
    let Some(element) = ElementRef::new(tree, element) else {
        return false;
    };
    let mut caches = SelectorCaches::default();
    let quirks = match tree.quirks {
        true => QuirksMode::Quirks,
        false => QuirksMode::NoQuirks,
    };
    let mut context = MatchingContext::new(
        MatchingMode::Normal,
        None,
        &mut caches,
        quirks,
        NeedsSelectorFlags::No,
        MatchingForInvalidation::No,
    );
    context.scope_element = ElementRef::new(tree, scope).map(|scope| scope.opaque());
    matching::matches_selector_list(selectors, &element, &mut context)
}

/// `querySelector`: the first element inside `root`, in tree order, that matches.
pub(crate) fn query(tree: &Tree, root: NodeId, selectors: &Selectors) -> Option<NodeId> {
    let mut stack: Vec<NodeId> = tree.children(root).collect();
    stack.reverse();
    while let Some(node) = stack.pop() {
        if matches(tree, node, selectors, root) {
            return Some(node);
        }
        let start = stack.len();
        stack.extend(tree.children(node));
        stack[start..].reverse();
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Impl;

impl parser::SelectorImpl for Impl {
    type ExtraMatchingData<'a> = ();
    type AttrValue = CssString;
    type Identifier = CssLocalName;
    type LocalName = CssLocalName;
    type NamespacePrefix = CssLocalName;
    type NamespaceUrl = Namespace;
    type BorrowedNamespaceUrl = Namespace;
    type BorrowedLocalName = CssLocalName;
    type NonTSPseudoClass = PseudoClass;
    type PseudoElement = PseudoElement;
}

struct SelectorParser;

impl<'i> parser::Parser<'i> for SelectorParser {
    type Impl = Impl;
    type Error = SelectorParseErrorKind<'i>;

    fn parse_is_and_where(&self) -> bool {
        true
    }

    fn parse_has(&self) -> bool {
        true
    }

    fn parse_nth_child_of(&self) -> bool {
        true
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CssString(String);

impl From<&str> for CssString {
    fn from(value: &str) -> Self {
        CssString(value.to_owned())
    }
}

impl AsRef<str> for CssString {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl ToCss for CssString {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        cssparser::serialize_string(&self.0, dest)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CssLocalName(LocalName);

impl From<&str> for CssLocalName {
    fn from(value: &str) -> Self {
        CssLocalName(value.into())
    }
}

impl ToCss for CssLocalName {
    fn to_css<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
        dest.write_str(&self.0)
    }
}

impl PrecomputedHash for CssLocalName {
    fn precomputed_hash(&self) -> u32 {
        self.0.precomputed_hash()
    }
}

/// The pseudo-classes that aren't about the tree, of which none is supported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PseudoClass {}

impl parser::NonTSPseudoClass for PseudoClass {
    type Impl = Impl;

    fn is_active_or_hover(&self) -> bool {
        match *self {}
    }

    fn is_user_action_state(&self) -> bool {
        match *self {}
    }
}

impl ToCss for PseudoClass {
    fn to_css<W: fmt::Write>(&self, _: &mut W) -> fmt::Result {
        match *self {}
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PseudoElement {}

impl parser::PseudoElement for PseudoElement {
    type Impl = Impl;
}

impl ToCss for PseudoElement {
    fn to_css<W: fmt::Write>(&self, _: &mut W) -> fmt::Result {
        match *self {}
    }
}

#[derive(Clone, Copy)]
struct ElementRef<'t> {
    tree: &'t Tree,
    id: NodeId,
}

impl fmt::Debug for ElementRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<{}>", self.element().qualified_name())
    }
}

impl<'t> ElementRef<'t> {
    fn new(tree: &'t Tree, id: NodeId) -> Option<Self> {
        tree.element(id).map(|_| ElementRef { tree, id })
    }

    fn element(&self) -> &'t crate::tree::Element {
        self.tree.element(self.id).expect("an element")
    }

    fn sibling_element(&self, next: bool) -> Option<Self> {
        let step = |id: NodeId| {
            let node = self.tree.node(id);
            if next { node.next } else { node.previous }
        };
        let mut sibling = step(self.id);
        while let Some(id) = sibling {
            if let Some(element) = ElementRef::new(self.tree, id) {
                return Some(element);
            }
            sibling = step(id);
        }
        None
    }
}

impl Element for ElementRef<'_> {
    type Impl = Impl;

    fn opaque(&self) -> OpaqueElement {
        OpaqueElement::new(self.tree.node(self.id))
    }

    fn parent_element(&self) -> Option<Self> {
        ElementRef::new(self.tree, self.tree.node(self.id).parent?)
    }

    fn parent_node_is_shadow_root(&self) -> bool {
        false
    }

    fn containing_shadow_host(&self) -> Option<Self> {
        None
    }

    fn is_pseudo_element(&self) -> bool {
        false
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        self.sibling_element(false)
    }

    fn next_sibling_element(&self) -> Option<Self> {
        self.sibling_element(true)
    }

    fn first_element_child(&self) -> Option<Self> {
        let mut children = self.tree.children(self.id);
        children.find_map(|child| ElementRef::new(self.tree, child))
    }

    fn is_html_element_in_html_document(&self) -> bool {
        self.element().is_html()
    }

    fn has_local_name(&self, name: &CssLocalName) -> bool {
        self.element().name.local == name.0
    }

    fn has_namespace(&self, namespace: &Namespace) -> bool {
        self.element().name.ns == *namespace
    }

    fn is_same_type(&self, other: &Self) -> bool {
        let (a, b) = (&self.element().name, &other.element().name);
        a.local == b.local && a.ns == b.ns
    }

    fn attr_matches(
        &self,
        namespace: &NamespaceConstraint<&Namespace>,
        local_name: &CssLocalName,
        operation: &AttrSelectorOperation<&CssString>,
    ) -> bool {
        self.element().attrs.iter().any(|attr| {
            let in_namespace = match namespace {
                NamespaceConstraint::Any => true,
                NamespaceConstraint::Specific(namespace) => attr.name.ns == **namespace,
            };
            in_namespace && attr.name.local == local_name.0 && operation.eval_str(&attr.value)
        })
    }

    fn match_non_ts_pseudo_class(
        &self,
        pseudo_class: &PseudoClass,
        _: &mut MatchingContext<Impl>,
    ) -> bool {
        match *pseudo_class {}
    }

    fn match_pseudo_element(&self, _: &PseudoElement, _: &mut MatchingContext<Impl>) -> bool {
        false
    }

    fn apply_selector_flags(&self, _: ElementSelectorFlags) {}

    fn is_link(&self) -> bool {
        let element = self.element();
        element.is_html()
            && matches!(
                element.name.local,
                local_name!("a") | local_name!("area") | local_name!("link")
            )
            && element
                .attrs
                .iter()
                .any(|attr| attr.name.ns == ns!() && attr.name.local == local_name!("href"))
    }

    fn is_html_slot_element(&self) -> bool {
        let element = self.element();
        element.is_html() && element.name.local == local_name!("slot")
    }

    fn has_id(&self, id: &CssLocalName, case_sensitivity: CaseSensitivity) -> bool {
        let value = self
            .element()
            .attrs
            .iter()
            .find(|attr| attr.name.ns == ns!() && attr.name.local == local_name!("id"));
        value.is_some_and(|attr| case_sensitivity.eq(id.0.as_bytes(), attr.value.as_bytes()))
    }

    fn has_class(&self, name: &CssLocalName, case_sensitivity: CaseSensitivity) -> bool {
        let class = self
            .element()
            .attrs
            .iter()
            .find(|attr| attr.name.ns == ns!() && attr.name.local == local_name!("class"));
        class.is_some_and(|attr| {
            let mut classes = attr.value.split(|c: char| c.is_ascii_whitespace());
            classes.any(|class| case_sensitivity.eq(name.0.as_bytes(), class.as_bytes()))
        })
    }

    fn has_custom_state(&self, _: &CssLocalName) -> bool {
        false
    }

    fn imported_part(&self, _: &CssLocalName) -> Option<CssLocalName> {
        None
    }

    fn is_part(&self, _: &CssLocalName) -> bool {
        false
    }

    fn is_empty(&self) -> bool {
        !self
            .tree
            .children(self.id)
            .any(|child| match &self.tree.node(child).data {
                Data::Element(_) => true,
                Data::Text(text) => !text.is_empty(),
                _ => false,
            })
    }

    fn is_root(&self) -> bool {
        let parent = self.tree.node(self.id).parent;
        parent.is_some_and(|parent| matches!(self.tree.node(parent).data, Data::Document))
    }

    fn add_element_unique_hashes(&self, _: &mut BloomFilter) -> bool {
        false
    }
}
