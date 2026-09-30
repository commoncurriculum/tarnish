//! Serializing documents: `DOMSerializer`. One walk over the document renders each node's and
//! mark's spec to a [`Target`]: a DOM, or anything else that takes them in document order, such
//! as HTML written as it goes.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::{Dom, NodeKind};
use crate::chunk::{Kind, ValueRef};
use crate::js;
use crate::js::stack;
use crate::js::value::Nullish;
use crate::json::{Map, Value};
use crate::model::{Fragment, Mark, Node, TextRef};
use crate::{Error, Result};

/// What a node's or mark's `toDOM` gives: ProseMirror's `DOMOutputSpec`.
#[derive(Clone)]
pub enum DomSpec<'a, N> {
    /// A DOM element to use as it is.
    Node(N),
    /// An element, and the element in it to put the content in: `{dom, contentDOM}`.
    Rendered(Rendered<N>),
    /// `[name, attrs?, ...children]`, with DOM nodes among its items. For an array made from
    /// one in the node's or mark's attributes, `origin` is that array's [`ValueRef::id`]: such
    /// an array is refused when it starts with a string, as an attacker may have written it.
    /// Only this origin is checked.
    Array {
        items: Vec<DomSpec<'a, N>>,
        origin: Option<(usize, u32)>,
    },
    /// A string, the content hole `0` or an attributes object. An array is read as a
    /// [`DomSpec::Array`] with no origin.
    Value(Value),
    /// `[tag, attrs, ...children]`, setting its attributes in order.
    Element {
        tag: &'a str,
        attrs: SpecAttrs<'a>,
        children: Vec<DomSpec<'a, N>>,
    },
    /// `[tag, attrs, 0]`: an element the content goes in.
    Wrapping { tag: &'a str, attrs: SpecAttrs<'a> },
    /// A string, which renders as text.
    Text(Cow<'a, str>),
    /// `0`, where the content goes.
    Hole,
    /// A value of the node's or mark's attributes, which JSON can make anything: text when it's
    /// a string, and otherwise what `renderSpec` makes of it.
    Attr(ValueRef<'a>),
}

impl<N> From<Value> for DomSpec<'_, N> {
    fn from(value: Value) -> Self {
        if !matches!(value, Value::Array(_)) {
            return DomSpec::Value(value);
        }
        let items = value.into_array().expect("an array");
        DomSpec::Array {
            items: items
                .into_iter()
                .map(|item| stack::grow(|| item.into()))
                .collect(),
            origin: None,
        }
    }
}

/// An array read from JSON nests as deeply as the JSON does, so each level drops on the stack
/// segments `stack::grow` adds.
impl<N> Drop for DomSpec<'_, N> {
    fn drop(&mut self) {
        if let DomSpec::Array { items, .. } = self {
            stack::drop_nested(items);
        }
    }
}

impl<'a, N> DomSpec<'a, N> {
    /// `[tag, attrs, ...children]`.
    pub fn element(tag: &'a str, attrs: SpecAttrs<'a>, children: Vec<DomSpec<'a, N>>) -> Self {
        DomSpec::Element {
            tag,
            attrs,
            children,
        }
    }

    /// `[tag, attrs, 0]`.
    pub fn wrapping(tag: &'a str, attrs: SpecAttrs<'a>) -> Self {
        DomSpec::Wrapping { tag, attrs }
    }

    /// `value ?? fallback`, `None` being `undefined`.
    pub fn attr_or(value: impl Into<Option<ValueRef<'a>>>, fallback: &'a str) -> Self {
        match value.into().filter(|value| !value.is_null()) {
            Some(value) => DomSpec::Attr(value),
            None => DomSpec::Text(Cow::Borrowed(fallback)),
        }
    }
}

/// A rendered spec: its element, and the element to put the content in, if it has a hole.
#[derive(Clone)]
pub struct Rendered<N> {
    pub dom: N,
    pub content_dom: Option<N>,
}

/// The value of an attribute a spec sets: one of the node's or mark's attributes, or text.
#[derive(Clone)]
pub enum AttrValue<'a> {
    Json(ValueRef<'a>),
    Text(Cow<'a, str>),
}

impl AttrValue<'_> {
    /// Whether it's `null`, which a spec's attributes leave unset.
    pub fn is_null(&self) -> bool {
        matches!(self, AttrValue::Json(value) if value.is_null())
    }

    pub fn truthy(&self) -> bool {
        match self {
            AttrValue::Json(value) => value.truthy(),
            AttrValue::Text(text) => !text.is_empty(),
        }
    }

    /// `String(value)`, as `setAttribute` stores it.
    pub fn to_js_string(&self) -> Result<Cow<'_, str>> {
        match self {
            AttrValue::Json(value) => value.to_js_string(),
            AttrValue::Text(text) => Ok(Cow::Borrowed(text)),
        }
    }

    /// The value, when it's a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            AttrValue::Json(value) => value.as_str(),
            AttrValue::Text(text) => Some(text),
        }
    }

    fn to_value(&self) -> Value {
        match self {
            AttrValue::Json(value) => value.to_value(),
            AttrValue::Text(text) => Value::String(text.as_ref().into()),
        }
    }
}

impl<'a> From<ValueRef<'a>> for AttrValue<'a> {
    fn from(value: ValueRef<'a>) -> Self {
        AttrValue::Json(value)
    }
}

impl<'a> From<&'a str> for AttrValue<'a> {
    fn from(text: &'a str) -> Self {
        AttrValue::Text(Cow::Borrowed(text))
    }
}

impl From<String> for AttrValue<'_> {
    fn from(text: String) -> Self {
        AttrValue::Text(Cow::Owned(text))
    }
}

/// A spec's attributes, in the order they were first set. A name the spec can't borrow, such as
/// one an attribute's rendering makes, is its own.
#[derive(Clone, Default)]
pub struct SpecAttrs<'a>(Vec<(Cow<'a, str>, AttrValue<'a>)>);

impl<'a> SpecAttrs<'a> {
    pub fn new() -> Self {
        SpecAttrs(Vec::new())
    }

    pub fn get(&self, name: &str) -> Option<&AttrValue<'a>> {
        self.0
            .iter()
            .find(|(set, _)| set == name)
            .map(|(_, value)| value)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut AttrValue<'a>> {
        self.0
            .iter_mut()
            .find(|(set, _)| set == name)
            .map(|(_, value)| value)
    }

    /// `attrs[name] = value`.
    pub fn set(&mut self, name: impl Into<Cow<'a, str>>, value: impl Into<AttrValue<'a>>) {
        let (name, value) = (name.into(), value.into());
        match self.get_mut(&name) {
            Some(set) => *set = value,
            None => self.0.push((name, value)),
        }
    }

    /// Sets an attribute not yet set.
    pub fn push(&mut self, name: impl Into<Cow<'a, str>>, value: AttrValue<'a>) {
        let name = name.into();
        debug_assert!(self.get(&name).is_none(), "{name} is set");
        self.0.push((name, value));
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (&str, &AttrValue<'a>)> {
        self.0.iter().map(|(name, value)| (name.as_ref(), value))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'a, const N: usize> From<[(&'a str, AttrValue<'a>); N]> for SpecAttrs<'a> {
    fn from(entries: [(&'a str, AttrValue<'a>); N]) -> Self {
        SpecAttrs(
            entries
                .into_iter()
                .map(|(name, value)| (Cow::Borrowed(name), value))
                .collect(),
        )
    }
}

/// A node's `toDOM`, whose spec may borrow from the node.
pub type NodeToDom<N> =
    Arc<dyn for<'n> Fn(&'n Node<'static>) -> Result<DomSpec<'n, N>> + Send + Sync>;

/// A mark's `toDOM`, told whether the mark's content is inline, whose spec may borrow from the
/// mark.
pub type MarkToDom<N> =
    Arc<dyn for<'m> Fn(&'m Mark<'static>, bool) -> Result<DomSpec<'m, N>> + Send + Sync>;

/// A node's `toDOM` of a function, which Rust can't infer from [`NodeToDom`] for a closure
/// whose spec borrows from the node.
pub fn node_to_dom<N, F>(to_dom: F) -> NodeToDom<N>
where
    F: for<'n> Fn(&'n Node<'static>) -> Result<DomSpec<'n, N>> + Send + Sync + 'static,
{
    Arc::new(to_dom)
}

/// A mark's `toDOM` of a function, as [`node_to_dom`] makes a node's.
pub fn mark_to_dom<N, F>(to_dom: F) -> MarkToDom<N>
where
    F: for<'m> Fn(&'m Mark<'static>, bool) -> Result<DomSpec<'m, N>> + Send + Sync + 'static,
{
    Arc::new(to_dom)
}

/// Where a serializer puts what the specs render to, in document order.
pub trait Target<N> {
    /// Where the nodes after a mark go once the mark ends.
    type Parent;

    /// Renders a mark's spec where the next node goes, and makes its content where the nodes
    /// after it go. `attrs` are the mark's attributes.
    fn open_mark(&mut self, spec: DomSpec<'_, N>, attrs: ValueRef<'_>) -> Result<Self::Parent>;

    /// Ends the mark opened last: the nodes after it go in `parent`.
    fn close_mark(&mut self, parent: Self::Parent) -> Result<()>;

    /// Renders a node's spec where the next node goes, `content` filling its hole if it has
    /// one. `attrs` are the node's attributes.
    fn node(
        &mut self,
        spec: DomSpec<'_, N>,
        attrs: ValueRef<'_>,
        content: impl FnOnce(&mut Self) -> Result<()>,
    ) -> Result<()>;

    fn text(&mut self, text: TextRef<'_>) -> Result<()>;
}

/// Serializes nodes and marks with each type's `toDOM`.
pub struct DomSerializer<N> {
    nodes: FxHashMap<String, NodeToDom<N>>,
    marks: FxHashMap<String, MarkToDom<N>>,
}

impl<N: Clone> DomSerializer<N> {
    /// A serializer of the node types and mark types named. A mark type left out isn't
    /// serialized.
    pub fn new(nodes: HashMap<String, NodeToDom<N>>, marks: HashMap<String, MarkToDom<N>>) -> Self {
        DomSerializer {
            nodes: nodes.into_iter().collect(),
            marks: marks.into_iter().collect(),
        }
    }

    /// Serialize the fragment's nodes into `target`, or a new document fragment.
    pub fn serialize_fragment<D: Dom<Node = N>>(
        &self,
        dom: &D,
        fragment: &Fragment<'static>,
        target: Option<N>,
    ) -> Result<N> {
        let target = match target {
            Some(target) => target,
            None => dom.create_fragment()?,
        };
        let mut into = DomTarget {
            dom,
            top: target.clone(),
        };
        self.write_fragment(fragment, &mut into)?;
        Ok(target)
    }

    /// `serializeFragment`'s walk, rendering to `target`.
    pub fn write_fragment<T: Target<N>>(
        &self,
        fragment: &Fragment<'static>,
        target: &mut T,
    ) -> Result<()> {
        let mut active: Vec<(Mark<'static>, T::Parent)> = Vec::new();
        for node in fragment.children() {
            let marks = node.marks();
            if !active.is_empty() || !marks.is_empty() {
                let (mut keep, mut rendered) = (0, 0);
                while keep < active.len() && rendered < marks.len() {
                    let next = marks.get(rendered).expect("a mark");
                    if !self.marks.contains_key(next.mark_type().name()) {
                        rendered += 1;
                        continue;
                    }
                    if next != active[keep].0 || next.mark_type().spec().spanning == Some(false) {
                        break;
                    }
                    keep += 1;
                    rendered += 1;
                }
                while keep < active.len() {
                    let (_, parent) = active.pop().expect("an active mark");
                    target.close_mark(parent)?;
                }
                while rendered < marks.len() {
                    let add = marks.get(rendered).expect("a mark");
                    rendered += 1;
                    if let Some(to_dom) = self.marks.get(add.mark_type().name()) {
                        let spec = to_dom(&add, node.is_inline())?;
                        let parent = target.open_mark(spec, add.attrs_view())?;
                        active.push((add, parent));
                    }
                }
            }
            match node.text() {
                Some(text) => target.text(text)?,
                None => target.node(self.spec(&node)?, node.attrs_view(), |target| {
                    self.fill(&node, |fragment| self.write_fragment(fragment, target))
                })?,
            }
        }
        while let Some((_, parent)) = active.pop() {
            target.close_mark(parent)?;
        }
        Ok(())
    }

    /// The node's spec from its type's `toDOM`.
    fn spec<'n>(&self, node: &'n Node<'static>) -> Result<DomSpec<'n, N>> {
        let to_dom = self
            .nodes
            .get(node.node_type().name())
            .ok_or_else(|| js::value::not_a_function("this.nodes[node.type.name]"))?;
        to_dom(node)
    }

    /// Serializes the node's content with `write`, into the hole its spec has.
    fn fill(
        &self,
        node: &Node<'static>,
        write: impl FnOnce(&Fragment<'static>) -> Result<()>,
    ) -> Result<()> {
        if node.is_leaf() {
            return Err(Error::Range(
                "Content hole not allowed in a leaf node spec".into(),
            ));
        }
        stack::grow(|| write(node.content()))
    }

    /// Serialize a node, with its marks around it.
    pub fn serialize_node<D: Dom<Node = N>>(&self, dom: &D, node: &Node<'static>) -> Result<N> {
        let mut element = match node.text() {
            Some(text) => dom.create_text(&text.to_text())?,
            None => {
                let rendered = render(dom, &self.spec(node)?, None, Some(node.attrs_view()))?;
                if let Some(content_dom) = rendered.content_dom {
                    self.fill(node, |fragment| {
                        self.serialize_fragment(dom, fragment, Some(content_dom))
                            .map(drop)
                    })?;
                }
                rendered.dom
            }
        };
        for mark in node.marks().iter().rev() {
            if let Some(wrap) = self.serialize_mark(dom, &mark, node.is_inline())? {
                dom.append_child(wrap.content_dom.as_ref().unwrap_or(&wrap.dom), &element)?;
                element = wrap.dom;
            }
        }
        Ok(element)
    }

    /// The mark's DOM, `None` when marks of its type aren't serialized.
    pub fn serialize_mark<D: Dom<Node = N>>(
        &self,
        dom: &D,
        mark: &Mark<'static>,
        inline: bool,
    ) -> Result<Option<Rendered<N>>> {
        let Some(to_dom) = self.marks.get(mark.mark_type().name()) else {
            return Ok(None);
        };
        let spec = to_dom(mark, inline)?;
        render(dom, &spec, None, Some(mark.attrs_view())).map(Some)
    }
}

/// A DOM as a serializer's target: what it renders goes in `top`.
struct DomTarget<'d, D: Dom> {
    dom: &'d D,
    top: D::Node,
}

impl<D: Dom> Target<D::Node> for DomTarget<'_, D> {
    type Parent = D::Node;

    fn open_mark(&mut self, spec: DomSpec<'_, D::Node>, attrs: ValueRef<'_>) -> Result<D::Node> {
        let rendered = render(self.dom, &spec, None, Some(attrs))?;
        self.dom.append_child(&self.top, &rendered.dom)?;
        let content = rendered.content_dom.unwrap_or(rendered.dom);
        Ok(std::mem::replace(&mut self.top, content))
    }

    fn close_mark(&mut self, parent: D::Node) -> Result<()> {
        self.top = parent;
        Ok(())
    }

    fn node(
        &mut self,
        spec: DomSpec<'_, D::Node>,
        attrs: ValueRef<'_>,
        content: impl FnOnce(&mut Self) -> Result<()>,
    ) -> Result<()> {
        let rendered = render(self.dom, &spec, None, Some(attrs))?;
        if let Some(content_dom) = rendered.content_dom {
            let parent = std::mem::replace(&mut self.top, content_dom);
            let filled = content(self);
            self.top = parent;
            filled?;
        }
        self.dom.append_child(&self.top, &rendered.dom)
    }

    fn text(&mut self, text: TextRef<'_>) -> Result<()> {
        let text = self.dom.create_text(&text.to_text())?;
        self.dom.append_child(&self.top, &text)
    }
}

/// `DOMSerializer.renderSpec`: render a spec, a string as a text node. With a hole in the spec,
/// `content_dom` is the element that has it.
pub fn render_spec<D: Dom>(
    dom: &D,
    structure: &DomSpec<D::Node>,
    xml_ns: Option<&str>,
) -> Result<Rendered<D::Node>> {
    render(dom, structure, xml_ns, None)
}

/// `renderSpec` of a node's or mark's spec, which refuses an array from `attrs` that starts with
/// a string.
pub fn render_spec_of<D: Dom>(
    dom: &D,
    structure: &DomSpec<D::Node>,
    attrs: ValueRef,
) -> Result<Rendered<D::Node>> {
    render(dom, structure, None, Some(attrs))
}

fn invalid() -> Error {
    Error::Range("Invalid array passed to renderSpec".into())
}

fn hole_not_alone() -> Error {
    Error::Range("Content hole must be the only child of its parent node".into())
}

fn suspicious() -> Error {
    Error::Range(
        "Using an array from an attribute object as a DOM spec. This may be an attempted cross site scripting attack.".into(),
    )
}

/// `renderSpec` of a value from a node's or mark's attributes, which JSON can make anything: a
/// string renders as text, and an array, or an object read as one by its indices and `length`,
/// as an element. No JSON object is a DOM node, so one `renderSpec` takes for a node is refused
/// as `appendChild` refuses it.
fn render_attr<D: Dom>(
    dom: &D,
    structure: ValueRef,
    xml_ns: Option<&str>,
    block_arrays_in: Option<ValueRef>,
) -> Result<Rendered<D::Node>> {
    let array = match structure.kind() {
        Kind::String(string) => return text(dom, string),
        Kind::Null => return Err(js::value::cannot_read(Nullish::Null, "nodeType")),
        Kind::Array(_) => true,
        Kind::Object(_) => false,
        Kind::Bool(_) | Kind::Number(_) => return Err(invalid()),
    };
    let is_node = |value: ValueRef| -> Result<bool> {
        Ok(value.is_object()
            && value
                .get("nodeType")
                .map_or(Ok(f64::NAN), ValueRef::to_number)?
                == 1.0)
    };
    if is_node(structure)? || structure.get("dom").map_or(Ok(false), is_node)? {
        return Err(Error::Type(
            "Failed to execute 'appendChild' on 'Node': parameter 1 is not of type 'Node'.".into(),
        ));
    }
    let item = |index: usize| match array {
        true => structure.items().nth(index),
        false => structure.get(&index.to_string()),
    };
    let Some(tag) = item(0).and_then(ValueRef::as_str) else {
        return Err(invalid());
    };
    if array
        && let Some(attrs) = block_arrays_in
        && holds_spec_array(attrs, structure.id())
    {
        return Err(suspicious());
    }
    let (namespace, tag) = qualified(tag, xml_ns);
    let element = dom.create_element(namespace, tag)?;
    let mut start = 1;
    if let Some(attrs) = item(1)
        && attrs.is_object()
        && attrs.get("nodeType").is_none_or(ValueRef::is_null)
    {
        start = 2;
        for (name, value) in attrs.entries() {
            if !value.is_null() {
                set_attribute(dom, &element, name, &value.to_value())?;
            }
        }
    }
    let length = match array {
        true => structure.len() as f64,
        false => structure
            .get("length")
            .map_or(Ok(f64::NAN), ValueRef::to_number)?,
    };
    let mut content_dom = None;
    let mut index = start;
    while (index as f64) < length {
        let child =
            item(index).ok_or_else(|| js::value::cannot_read(Nullish::Undefined, "nodeType"))?;
        if child.as_f64() == Some(0.0) {
            if (index as f64) < length - 1.0 || index > start {
                return Err(hole_not_alone());
            }
            return Ok(Rendered {
                dom: element.clone(),
                content_dom: Some(element),
            });
        }
        let inner = stack::grow(|| render_attr(dom, child, namespace, block_arrays_in))?;
        dom.append_child(&element, &inner.dom)?;
        if let Some(inner_content) = inner.content_dom {
            if content_dom.is_some() {
                return Err(Error::Range("Multiple content holes".into()));
            }
            content_dom = Some(inner_content);
        }
        index += 1;
    }
    Ok(Rendered {
        dom: element,
        content_dom,
    })
}

fn text<D: Dom>(dom: &D, text: &str) -> Result<Rendered<D::Node>> {
    Ok(Rendered {
        dom: dom.create_text(&text.into())?,
        content_dom: None,
    })
}

fn render<D: Dom>(
    dom: &D,
    structure: &DomSpec<D::Node>,
    xml_ns: Option<&str>,
    block_arrays_in: Option<ValueRef>,
) -> Result<Rendered<D::Node>> {
    let (items, origin) = match structure {
        DomSpec::Node(node) if dom.kind(node)? == NodeKind::Element => {
            return Ok(Rendered {
                dom: node.clone(),
                content_dom: None,
            });
        }
        DomSpec::Rendered(rendered) if dom.kind(&rendered.dom)? == NodeKind::Element => {
            return Ok(rendered.clone());
        }
        DomSpec::Text(string) => return text(dom, string),
        DomSpec::Value(Value::String(string)) => return text(dom, string),
        DomSpec::Attr(value) => return render_attr(dom, *value, xml_ns, block_arrays_in),
        DomSpec::Element {
            tag,
            attrs,
            children,
        } => {
            let (namespace, tag) = qualified(tag, xml_ns);
            let element = dom.create_element(namespace, tag)?;
            set_spec_attributes(dom, &element, attrs)?;
            return fill(dom, element, children, namespace, block_arrays_in);
        }
        DomSpec::Wrapping { tag, attrs } => {
            let (namespace, tag) = qualified(tag, xml_ns);
            let element = dom.create_element(namespace, tag)?;
            set_spec_attributes(dom, &element, attrs)?;
            return Ok(Rendered {
                dom: element.clone(),
                content_dom: Some(element),
            });
        }
        DomSpec::Array { items, origin } => (items, *origin),
        DomSpec::Value(array @ Value::Array(_)) => {
            return render(dom, &array.clone().into(), xml_ns, block_arrays_in);
        }
        _ => return Err(invalid()),
    };
    let tag = match items.first() {
        Some(DomSpec::Value(Value::String(tag))) => tag.as_str(),
        Some(DomSpec::Text(tag)) => tag.as_ref(),
        _ => return Err(invalid()),
    };
    if let (Some(attrs), Some(origin)) = (block_arrays_in, origin)
        && holds_spec_array(attrs, origin)
    {
        return Err(suspicious());
    }
    let (namespace, tag) = qualified(tag, xml_ns);
    let element = dom.create_element(namespace, tag)?;
    // Any object that is neither an array nor a DOM node holds the attributes, a
    // `{dom, contentDOM}` one included.
    let start = match items.get(1) {
        Some(DomSpec::Rendered(rendered)) => {
            let nodes = [
                ("dom", Some(&rendered.dom)),
                ("contentDOM", rendered.content_dom.as_ref()),
            ];
            for (name, node) in nodes {
                let Some(node) = node else { continue };
                let value = Value::String(dom.attribute_value(node)?);
                dom.set_attribute(&element, None, name, &value)?;
            }
            2
        }
        Some(DomSpec::Value(Value::Object(attrs))) => {
            set_attributes(dom, &element, attrs)?;
            2
        }
        _ => 1,
    };
    fill(
        dom,
        element,
        &items[start.min(items.len())..],
        namespace,
        block_arrays_in,
    )
}

/// Renders a spec's children into its element, which a hole must be the only one of.
fn fill<D: Dom>(
    dom: &D,
    element: D::Node,
    children: &[DomSpec<D::Node>],
    namespace: Option<&str>,
    block_arrays_in: Option<ValueRef>,
) -> Result<Rendered<D::Node>> {
    let mut content_dom = None;
    for child in children {
        if is_hole(child) {
            if children.len() > 1 {
                return Err(hole_not_alone());
            }
            return Ok(Rendered {
                dom: element.clone(),
                content_dom: Some(element),
            });
        }
        let inner = stack::grow(|| render(dom, child, namespace, block_arrays_in))?;
        dom.append_child(&element, &inner.dom)?;
        if let Some(inner_content) = inner.content_dom {
            if content_dom.is_some() {
                return Err(Error::Range("Multiple content holes".into()));
            }
            content_dom = Some(inner_content);
        }
    }
    Ok(Rendered {
        dom: element,
        content_dom,
    })
}

/// `child === 0`.
pub fn is_hole<N>(child: &DomSpec<N>) -> bool {
    match child {
        DomSpec::Hole => true,
        DomSpec::Value(Value::Number(number)) => number.as_f64() == Some(0.0),
        DomSpec::Attr(value) => value.as_f64() == Some(0.0),
        _ => false,
    }
}

/// A spec's tag and its namespace: the part before a space, or the namespace it's in.
pub fn qualified<'t>(tag: &'t str, xml_ns: Option<&'t str>) -> (Option<&'t str>, &'t str) {
    match tag.find(' ') {
        Some(space) if space > 0 => (Some(&tag[..space]), &tag[space + 1..]),
        _ => (xml_ns, tag),
    }
}

fn set_attribute<D: Dom>(dom: &D, element: &D::Node, name: &str, value: &Value) -> Result<()> {
    match name.find(' ') {
        Some(space) if space > 0 => {
            dom.set_attribute(element, Some(&name[..space]), &name[space + 1..], value)
        }
        _ if name == "style" && dom.set_style(element, value)? => Ok(()),
        _ => dom.set_attribute(element, None, name, value),
    }
}

fn set_attributes<D: Dom>(dom: &D, element: &D::Node, attrs: &Map) -> Result<()> {
    for (name, value) in attrs.iter() {
        if !matches!(value, Value::Null) {
            set_attribute(dom, element, name, value)?;
        }
    }
    Ok(())
}

fn set_spec_attributes<D: Dom>(dom: &D, element: &D::Node, attrs: &SpecAttrs) -> Result<()> {
    for (name, value) in attrs.iter() {
        if !value.is_null() {
            set_attribute(dom, element, name, &value.to_value())?;
        }
    }
    Ok(())
}

/// Whether the attributes hold the array `origin` where it could be taken for a spec: starting
/// with a string, and not inside another such array.
fn holds_spec_array(attrs: ValueRef, origin: (usize, u32)) -> bool {
    fn scan(value: ValueRef, origin: (usize, u32)) -> bool {
        let mut items = value.items();
        match items.next() {
            Some(first) if first.as_str().is_some() => value.id() == origin,
            Some(first) => {
                stack::grow(|| scan(first, origin))
                    || items.any(|item| stack::grow(|| scan(item, origin)))
            }
            None => value
                .entries()
                .any(|(_, item)| stack::grow(|| scan(item, origin))),
        }
    }
    attrs.entries().any(|(_, value)| scan(value, origin))
}
