//! Serializing documents to a DOM: `DOMSerializer`.

use std::collections::HashMap;
use std::sync::Arc;

use super::{Dom, NodeKind};
use crate::chunk::ValueRef;
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
    /// `[name, attrs?, ...children]`, with DOM nodes among its items. For an array made from
    /// one in the node's or mark's attributes, `origin` is that array's [`ValueRef::id`]: such
    /// an array is refused when it starts with a string, as an attacker may have written it.
    /// Only this origin is checked.
    Array {
        items: Vec<DomSpec<N>>,
        origin: Option<(usize, u32)>,
    },
    /// A string, the content hole `0` or an attributes object. An array is read as a
    /// [`DomSpec::Array`] with no origin.
    Value(Value),
}

impl<N> From<Value> for DomSpec<N> {
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

/// A rendered spec: its element, and the element to put the content in, if it has a hole.
#[derive(Clone)]
pub struct Rendered<N> {
    pub dom: N,
    pub content_dom: Option<N>,
}

pub type NodeToDom<N> = Arc<dyn Fn(&Node<'static>) -> Result<DomSpec<N>> + Send + Sync>;

/// A mark's `toDOM`, told whether the mark's content is inline.
pub type MarkToDom<N> = Arc<dyn Fn(&Mark<'static>, bool) -> Result<DomSpec<N>> + Send + Sync>;

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
        fragment: &Fragment<'static>,
        target: Option<N>,
    ) -> Result<N> {
        let target = match target {
            Some(target) => target,
            None => dom.create_fragment()?,
        };
        let mut top = target.clone();
        let mut active: Vec<(Mark<'static>, N)> = Vec::new();
        for node in fragment.children() {
            let marks = node.marks().to_vec();
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
            let inner = self.serialize_node_inner(dom, &node)?;
            dom.append_child(&top, &inner)?;
        }
        Ok(target)
    }

    fn serialize_node_inner<D: Dom<Node = N>>(&self, dom: &D, node: &Node<'static>) -> Result<N> {
        if let Some(text) = node.text() {
            return dom.create_text(&text.to_text());
        }
        let name = node.node_type().name();
        let to_dom = self
            .nodes
            .get(name)
            .ok_or_else(|| Error::Other(format!("No toDOM for node type {name}")))?;
        let spec = to_dom(node)?;
        let rendered = render(dom, &spec, None, Some(node.attrs_view()))?;
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
    pub fn serialize_node<D: Dom<Node = N>>(&self, dom: &D, node: &Node<'static>) -> Result<N> {
        let mut element = self.serialize_node_inner(dom, node)?;
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

fn invalid() -> Error {
    Error::Range("Invalid array passed to renderSpec".into())
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
        DomSpec::Array { items, origin } => (items, *origin),
        DomSpec::Value(array @ Value::Array(_)) => {
            return render(dom, &array.clone().into(), xml_ns, block_arrays_in);
        }
        _ => return Err(invalid()),
    };
    let Some(DomSpec::Value(Value::String(tag))) = items.first() else {
        return Err(invalid());
    };
    if let (Some(attrs), Some(origin)) = (block_arrays_in, origin)
        && holds_spec_array(attrs, origin)
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
                let value = Value::String(dom.stringify(node)?);
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
    let mut content_dom = None;
    for (index, child) in items.iter().enumerate().skip(start) {
        match child {
            DomSpec::Value(Value::Number(number)) if number.as_f64() == Some(0.0) => {
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
            DomSpec::Value(Value::String(text)) => {
                let text = dom.create_text(&text.as_str().into())?;
                dom.append_child(&element, &text)?;
            }
            _ => {
                let inner = stack::grow(|| render(dom, child, namespace, block_arrays_in))?;
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

fn set_attributes<D: Dom>(dom: &D, element: &D::Node, attrs: &Map) -> Result<()> {
    for (name, value) in attrs.iter() {
        if matches!(value, Value::Null) {
            continue;
        }
        match name.find(' ') {
            Some(space) if space > 0 => {
                dom.set_attribute(element, Some(&name[..space]), &name[space + 1..], value)?
            }
            _ if name == "style" && dom.set_style(element, value)? => {}
            _ => dom.set_attribute(element, None, name, value)?,
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
