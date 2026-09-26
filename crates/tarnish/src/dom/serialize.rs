//! Serializing documents to a DOM: `DOMSerializer`.

use std::collections::HashMap;
use std::sync::Arc;

use super::{Dom, NodeKind};
use crate::error::{Error, Result};
use crate::json::{Map, Value};
use crate::model::{Fragment, Mark, Node};
use crate::stack;

/// What a node's or mark's `toDOM` gives: ProseMirror's `DOMOutputSpec`.
#[derive(Clone)]
pub enum DomSpec<N> {
    /// A DOM element to use as it is.
    Node(N),
    /// An element, and the element in it to put the content in: `{dom, contentDOM}`.
    Rendered(Rendered<N>),
    /// `[name, attrs?, ...children]`, with DOM nodes among its items. `origin` identifies the
    /// array of an attribute value it is, if it is one: see [`DomSpec::Value`].
    Array {
        items: Vec<DomSpec<N>>,
        origin: Option<usize>,
    },
    /// A value: a string, the content hole `0`, an attributes object, or an array spec of
    /// values. An array taken from a node's or mark's attributes is refused, as it may be one
    /// an attacker wrote.
    Value(Value),
}

/// A rendered spec: its element, and the element to put the content in, if it has a hole.
#[derive(Clone)]
pub struct Rendered<N> {
    pub dom: N,
    pub content_dom: Option<N>,
}

pub type NodeToDom<N> = Arc<dyn Fn(&Node) -> Result<DomSpec<N>> + Send + Sync>;

/// A mark's `toDOM`, told whether the mark's content is inline.
pub type MarkToDom<N> = Arc<dyn Fn(&Mark, bool) -> Result<DomSpec<N>> + Send + Sync>;

/// Serializes nodes and marks to a DOM with each type's `toDOM`.
pub struct DomSerializer<N> {
    nodes: HashMap<String, NodeToDom<N>>,
    marks: HashMap<String, MarkToDom<N>>,
}

impl<N: Clone> DomSerializer<N> {
    /// A serializer of the node types and mark types named. A mark type left out isn't
    /// serialized.
    pub fn new(nodes: HashMap<String, NodeToDom<N>>, marks: HashMap<String, MarkToDom<N>>) -> Self {
        DomSerializer { nodes, marks }
    }

    /// Serialize the fragment's nodes into `target`, or a new document fragment.
    pub fn serialize_fragment<D: Dom<Node = N>>(
        &self,
        dom: &D,
        fragment: &Fragment,
        target: Option<N>,
    ) -> Result<N> {
        let target = match target {
            Some(target) => target,
            None => dom.create_fragment()?,
        };
        let mut top = target.clone();
        let mut active: Vec<(Mark, N)> = Vec::new();
        for node in fragment.children() {
            let marks = node.marks();
            if !active.is_empty() || !marks.is_empty() {
                let (mut keep, mut rendered) = (0, 0);
                while keep < active.len() && rendered < marks.len() {
                    let next = &marks[rendered];
                    if !self.marks.contains_key(next.mark_type().name()) {
                        rendered += 1;
                        continue;
                    }
                    if *next != active[keep].0 || next.mark_type().spec().spanning == Some(false) {
                        break;
                    }
                    keep += 1;
                    rendered += 1;
                }
                while keep < active.len() {
                    top = active.pop().expect("an active mark").1;
                }
                while rendered < marks.len() {
                    let add = &marks[rendered];
                    rendered += 1;
                    if let Some(mark_dom) = self.serialize_mark(dom, add, node.is_inline())? {
                        active.push((add.clone(), top.clone()));
                        dom.append_child(&top, &mark_dom.dom)?;
                        top = mark_dom.content_dom.unwrap_or(mark_dom.dom);
                    }
                }
            }
            let inner = self.serialize_node_inner(dom, node)?;
            dom.append_child(&top, &inner)?;
        }
        Ok(target)
    }

    fn serialize_node_inner<D: Dom<Node = N>>(&self, dom: &D, node: &Node) -> Result<N> {
        if let Some(text) = node.text() {
            return dom.create_text(text);
        }
        let name = node.node_type().name();
        let to_dom = self
            .nodes
            .get(name)
            .ok_or_else(|| Error::Other(format!("No toDOM for node type {name}")))?;
        let spec = to_dom(node)?;
        let rendered = render(dom, &spec, None, Some(node.attrs()))?;
        if let Some(content_dom) = rendered.content_dom {
            if node.is_leaf() {
                return Err(Error::Range(
                    "Content hole not allowed in a leaf node spec".into(),
                ));
            }
            stack::grow(|| self.serialize_fragment(dom, node.content(), Some(content_dom)))?;
        }
        Ok(rendered.dom)
    }

    /// Serialize a node, with its marks around it.
    pub fn serialize_node<D: Dom<Node = N>>(&self, dom: &D, node: &Node) -> Result<N> {
        let mut element = self.serialize_node_inner(dom, node)?;
        for mark in node.marks().iter().rev() {
            if let Some(wrap) = self.serialize_mark(dom, mark, node.is_inline())? {
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
        mark: &Mark,
        inline: bool,
    ) -> Result<Option<Rendered<N>>> {
        let Some(to_dom) = self.marks.get(mark.mark_type().name()) else {
            return Ok(None);
        };
        let spec = to_dom(mark, inline)?;
        render(dom, &spec, None, Some(mark.attrs())).map(Some)
    }
}

/// `DOMSerializer.renderSpec`: render a spec, a string as a text node. With a hole in the spec,
/// `content_dom` is the element that has it.
pub fn render_spec<D: Dom>(
    dom: &D,
    structure: &DomSpec<D::Node>,
    xml_ns: Option<&str>,
) -> Result<Rendered<D::Node>> {
    if let DomSpec::Value(Value::String(text)) = structure {
        return Ok(Rendered {
            dom: dom.create_text(&(**text).into())?,
            content_dom: None,
        });
    }
    render(dom, structure, xml_ns, None)
}

/// An item of an array spec.
enum Item<'a, N> {
    Spec(&'a DomSpec<N>),
    Value(&'a Value),
}

impl<'a, N: Clone> Item<'a, N> {
    fn spec(&self) -> DomSpec<N> {
        match self {
            Item::Spec(spec) => (*spec).clone(),
            Item::Value(value) => DomSpec::Value((*value).clone()),
        }
    }

    fn value(&self) -> Option<&'a Value> {
        match *self {
            Item::Spec(DomSpec::Value(value)) | Item::Value(value) => Some(value),
            Item::Spec(_) => None,
        }
    }
}

fn invalid() -> Error {
    Error::Range("Invalid array passed to renderSpec".into())
}

fn render<D: Dom>(
    dom: &D,
    structure: &DomSpec<D::Node>,
    xml_ns: Option<&str>,
    block_arrays_in: Option<&Map>,
) -> Result<Rendered<D::Node>> {
    let (items, origin): (Vec<Item<D::Node>>, Option<usize>) = match structure {
        DomSpec::Node(node) if dom.kind(node)? == NodeKind::Element => {
            return Ok(Rendered {
                dom: node.clone(),
                content_dom: None,
            });
        }
        DomSpec::Rendered(rendered) if dom.kind(&rendered.dom)? == NodeKind::Element => {
            return Ok(rendered.clone());
        }
        DomSpec::Array { items, origin } => (items.iter().map(Item::Spec).collect(), *origin),
        DomSpec::Value(Value::Array(items)) => (
            items.iter().map(Item::Value).collect(),
            Some(items.as_ptr() as usize),
        ),
        _ => return Err(invalid()),
    };
    let Some(Value::String(tag)) = items.first().and_then(Item::value) else {
        return Err(invalid());
    };
    if let (Some(attrs), Some(origin)) = (block_arrays_in, origin)
        && suspicious_arrays(attrs).contains(&origin)
    {
        return Err(Error::Range(
            "Using an array from an attribute object as a DOM spec. This may be an attempted cross site scripting attack.".into(),
        ));
    }
    let (namespace, tag) = match tag.find(' ') {
        Some(space) if space > 0 => (Some(&tag[..space]), &tag[space + 1..]),
        _ => (xml_ns, tag.as_str()),
    };
    let element = dom.create_element(namespace, tag)?;
    let mut start = 1;
    if let Some(Value::Object(attrs)) = items.get(1).and_then(Item::value) {
        start = 2;
        for (name, value) in attrs.iter() {
            if matches!(value, Value::Null) {
                continue;
            }
            match name.find(' ') {
                Some(space) if space > 0 => {
                    dom.set_attribute(&element, Some(&name[..space]), &name[space + 1..], value)?
                }
                _ if name == "style" && dom.set_style(&element, value)? => {}
                _ => dom.set_attribute(&element, None, name, value)?,
            }
        }
    }
    let mut content_dom = None;
    for (index, child) in items.iter().enumerate().skip(start) {
        match child.value() {
            Some(Value::Number(number)) if number.as_f64() == Some(0.0) => {
                if index < items.len() - 1 || index > start {
                    return Err(Error::Range(
                        "Content hole must be the only child of its parent node".into(),
                    ));
                }
                return Ok(Rendered {
                    dom: element.clone(),
                    content_dom: Some(element),
                });
            }
            Some(Value::String(text)) => {
                let text = dom.create_text(&text.as_str().into())?;
                dom.append_child(&element, &text)?;
            }
            _ => {
                let inner = stack::grow(|| render(dom, &child.spec(), namespace, block_arrays_in))?;
                dom.append_child(&element, &inner.dom)?;
                if let Some(inner_content) = inner.content_dom {
                    if content_dom.is_some() {
                        return Err(Error::Range("Multiple content holes".into()));
                    }
                    content_dom = Some(inner_content);
                }
            }
        }
    }
    Ok(Rendered {
        dom: element,
        content_dom,
    })
}

/// The arrays in attribute values that start with a string, and so could be taken for specs.
fn suspicious_arrays(attrs: &Map) -> Vec<usize> {
    fn scan(value: &Value, found: &mut Vec<usize>) {
        match value {
            Value::Array(items) if matches!(items.first(), Some(Value::String(_))) => {
                found.push(items.as_ptr() as usize);
            }
            Value::Array(items) => items.iter().for_each(|item| scan(item, found)),
            Value::Object(object) => object.iter().for_each(|(_, item)| scan(item, found)),
            _ => {}
        }
    }
    let mut found = Vec::new();
    attrs.iter().for_each(|(_, value)| scan(value, &mut found));
    found
}
