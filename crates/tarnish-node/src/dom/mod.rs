//! The DOM parser and serializer, over the host's DOM: jsdom in the tests.

mod parse;
mod serialize;

use std::rc::Rc;

use napi::bindgen_prelude::{FnArgs, FromNapiValue, JsObjectValue, Object, Unknown};
use napi::{Env, Error, JsValue, Result, UnknownRef, ValueType};
use tarnish::dom::{Dom, NodeKind};
use tarnish::{Text, Value};

use crate::js;

/// A node of the host's DOM, kept alive while tarnish holds it.
#[derive(Clone)]
pub struct JsNode(Rc<NodeRef>);

/// `None` only once dropped: letting go of the reference takes it.
struct NodeRef(Option<UnknownRef<false>>);

impl Drop for NodeRef {
    fn drop(&mut self) {
        if let (Some(node), Ok(env)) = (self.0.take(), js::env()) {
            // This fails only when the environment is being torn down, taking the reference.
            let _ = node.unref(&env);
        }
    }
}

impl JsNode {
    fn new(value: Unknown) -> Result<JsNode> {
        let node = UnknownRef::from_unknown(value)?;
        Ok(JsNode(Rc::new(NodeRef(Some(node)))))
    }

    fn value<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        let node = self.0.0.as_ref();
        node.ok_or_else(|| Error::from_reason("A DOM node let go of"))?
            .get_value(env)
    }

    fn object<'env>(&self, env: &'env Env) -> Result<Object<'env>> {
        Object::from_unknown(self.value(env)?)
    }

    fn get<'env>(&self, env: &'env Env, key: &str) -> Result<Unknown<'env>> {
        js::get(&self.object(env)?, key)
    }
}

/// The value as a DOM node, when it is an object, as DOM properties give either a node or
/// `null`.
fn node_or_none(value: Unknown) -> Result<Option<JsNode>> {
    match value.get_type()? {
        ValueType::Object => JsNode::new(value).map(Some),
        _ => Ok(None),
    }
}

/// A value that may be a DOM node, as one, when it is an object with a `nodeType`.
fn dom_node(value: Unknown) -> Result<Option<JsNode>> {
    if value.get_type()? != ValueType::Object
        || js::is_nullish(&js::get(&Object::from_unknown(value)?, "nodeType")?)?
    {
        return Ok(None);
    }
    JsNode::new(value).map(Some)
}

/// The host's DOM, with the document that makes its new nodes, if there is one.
struct JsDom<'env> {
    document: Option<Object<'env>>,
}

impl JsDom<'_> {
    fn document(&self) -> Result<Object<'_>> {
        self.document
            .ok_or_else(|| Error::from_reason("No document to create DOM nodes with"))
    }

    fn style<'env>(env: &'env Env, node: &JsNode) -> Result<Option<Object<'env>>> {
        let style = node.get(env, "style")?;
        match style.get_type()? {
            ValueType::Object => Object::from_unknown(style).map(Some),
            _ => Ok(None),
        }
    }
}

impl Dom for JsDom<'_> {
    type Node = JsNode;

    fn kind(&self, node: &JsNode) -> tarnish::Result<NodeKind> {
        js::host(|env| {
            let kind = node.get(env, "nodeType")?;
            if kind.get_type()? != ValueType::Number {
                return Ok(NodeKind::Other);
            }
            Ok(match u32::from_unknown(kind)? {
                1 => NodeKind::Element,
                3 => NodeKind::Text,
                _ => NodeKind::Other,
            })
        })
    }

    fn node_name(&self, node: &JsNode) -> tarnish::Result<String> {
        js::host(|env| String::from_unknown(node.get(env, "nodeName")?))
    }

    fn text(&self, node: &JsNode) -> tarnish::Result<Text> {
        js::host(|env| js::text_from_js(node.get(env, "nodeValue")?))
    }

    fn namespace(&self, node: &JsNode) -> tarnish::Result<Option<String>> {
        js::host(|env| js::get_string(&node.object(env)?, "namespaceURI"))
    }

    fn parent(&self, node: &JsNode) -> tarnish::Result<Option<JsNode>> {
        js::host(|env| node_or_none(node.get(env, "parentNode")?))
    }

    fn first_child(&self, node: &JsNode) -> tarnish::Result<Option<JsNode>> {
        js::host(|env| node_or_none(node.get(env, "firstChild")?))
    }

    fn next_sibling(&self, node: &JsNode) -> tarnish::Result<Option<JsNode>> {
        js::host(|env| node_or_none(node.get(env, "nextSibling")?))
    }

    fn previous_sibling(&self, node: &JsNode) -> tarnish::Result<Option<JsNode>> {
        js::host(|env| node_or_none(node.get(env, "previousSibling")?))
    }

    fn child(&self, node: &JsNode, index: usize) -> tarnish::Result<Option<JsNode>> {
        js::host(|env| {
            let children = Object::from_unknown(node.get(env, "childNodes")?)?;
            node_or_none(children.get_element(index as u32)?)
        })
    }

    fn matches(&self, node: &JsNode, selector: &str) -> tarnish::Result<bool> {
        js::host(|env| js::call_method(&node.object(env)?, "matches", selector)?.coerce_to_bool())
    }

    fn query_selector(&self, node: &JsNode, selector: &str) -> tarnish::Result<Option<JsNode>> {
        js::host(|env| {
            node_or_none(js::call_method(
                &node.object(env)?,
                "querySelector",
                selector,
            )?)
        })
    }

    fn style_count(&self, node: &JsNode) -> tarnish::Result<usize> {
        js::host(|env| match JsDom::style(env, node)? {
            Some(style) => Ok(u32::from_unknown(js::get(&style, "length")?)? as usize),
            None => Ok(0),
        })
    }

    fn style_value(&self, node: &JsNode, property: &str) -> tarnish::Result<String> {
        js::host(|env| match JsDom::style(env, node)? {
            Some(style) => {
                String::from_unknown(js::call_method(&style, "getPropertyValue", property)?)
            }
            None => Ok(String::new()),
        })
    }

    fn same(&self, a: &JsNode, b: &JsNode) -> tarnish::Result<bool> {
        js::host(|env| env.strict_equals(a.value(env)?, b.value(env)?))
    }

    fn contains(&self, ancestor: &JsNode, node: &JsNode) -> tarnish::Result<bool> {
        js::host(|env| {
            js::call_method(&ancestor.object(env)?, "contains", node.value(env)?)?.coerce_to_bool()
        })
    }

    fn compare_document_position(&self, a: &JsNode, b: &JsNode) -> tarnish::Result<u16> {
        js::host(|env| {
            let position =
                js::call_method(&a.object(env)?, "compareDocumentPosition", b.value(env)?)?;
            Ok(u32::from_unknown(position)? as u16)
        })
    }

    fn append_child(&self, parent: &JsNode, child: &JsNode) -> tarnish::Result<()> {
        js::host(|env| {
            js::call_method(&parent.object(env)?, "appendChild", child.value(env)?)?;
            Ok(())
        })
    }

    fn create_element(&self, namespace: Option<&str>, name: &str) -> tarnish::Result<JsNode> {
        js::host(|_| {
            let document = self.document()?;
            let element = match namespace {
                Some(namespace) => js::call_method(
                    &document,
                    "createElementNS",
                    FnArgs::from((namespace, name)),
                )?,
                None => js::call_method(&document, "createElement", name)?,
            };
            JsNode::new(element)
        })
    }

    fn create_text(&self, text: &Text) -> tarnish::Result<JsNode> {
        js::host(|env| {
            let text = js::text_to_js(env, text)?;
            JsNode::new(js::call_method(&self.document()?, "createTextNode", text)?)
        })
    }

    fn create_fragment(&self) -> tarnish::Result<JsNode> {
        js::host(|_| {
            JsNode::new(js::call_method(
                &self.document()?,
                "createDocumentFragment",
                (),
            )?)
        })
    }

    fn set_attribute(
        &self,
        element: &JsNode,
        namespace: Option<&str>,
        name: &str,
        value: &Value,
    ) -> tarnish::Result<()> {
        js::host(|env| {
            let (element, value) = (element.object(env)?, js::value_to_js(env, value)?);
            match namespace {
                Some(namespace) => {
                    let args = FnArgs::from((namespace, name, value));
                    js::call_method(&element, "setAttributeNS", args)?
                }
                None => js::call_method(&element, "setAttribute", FnArgs::from((name, value)))?,
            };
            Ok(())
        })
    }

    fn set_style(&self, element: &JsNode, css: &Value) -> tarnish::Result<bool> {
        js::host(|env| {
            let style = element.get(env, "style")?;
            if !style.coerce_to_bool()? {
                return Ok(false);
            }
            Object::from_unknown(style)?.set("cssText", js::value_to_js(env, css)?)?;
            Ok(true)
        })
    }

    fn stringify(&self, node: &JsNode) -> tarnish::Result<String> {
        js::host(|env| js::coerce_to_string(&node.value(env)?))
    }
}
