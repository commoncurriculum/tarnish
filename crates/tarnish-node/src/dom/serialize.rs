//! The DOM serializer, and rendering DOM specs.

use std::collections::HashMap;
use std::sync::Arc;

use napi::bindgen_prelude::{FnArgs, FromNapiValue, Object, Unknown};
use napi::{Env, JsValue, Result, ValueType};
use napi_derive::napi;
use tarnish::dom::{DomSerializer, DomSpec, MarkToDom, NodeToDom, Rendered};
use tarnish::{Map, Value};

use super::{JsDom, JsNode, attribute_value, dom_node, is_node};
use crate::fragment::FragmentHandle;
use crate::js::{self, Hook, OrThrow};
use crate::mark::{self, MarkHandle};
use crate::node::{self, NodeHandle};

/// A DOM spec of JavaScript's.
fn spec(value: Unknown) -> Result<DomSpec<'static, JsNode>> {
    if let Some(node) = dom_node(value)? {
        return Ok(DomSpec::Node(node));
    }
    if value.get_type()? == ValueType::Object {
        if value.is_array()? {
            let mut items = Vec::new();
            for (index, item) in Vec::<Unknown>::from_unknown(value)?.into_iter().enumerate() {
                items.push(
                    if index == 1
                        && let Some(attrs) = attributes(item)?
                    {
                        DomSpec::Value(Value::Object(attrs))
                    } else {
                        spec(item)?
                    },
                );
            }
            let origin = js::array_origin(value);
            return Ok(DomSpec::Array { items, origin });
        }
        let object = Object::from_unknown(value)?;
        if let Some(dom) = dom_node(js::get(&object, "dom")?)? {
            let content_dom = dom_node(js::get(&object, "contentDOM")?)?;
            return Ok(DomSpec::Rendered(Rendered { dom, content_dom }));
        }
    }
    Ok(DomSpec::Value(js::json_from_js(value)?))
}

/// The attributes an array spec's second item sets, when it is an object that is neither an
/// array nor a DOM node, even one with a `dom`. Each value is taken as the string that setting
/// an attribute turns it into, as it may be one JSON can't hold, such as a DOM node.
fn attributes(value: Unknown) -> Result<Option<Map>> {
    if value.get_type()? != ValueType::Object || value.is_array()? {
        return Ok(None);
    }
    let object = Object::from_unknown(value)?;
    if is_node(&object)? {
        return Ok(None);
    }
    let mut attrs = Map::new();
    for name in Object::keys(&object)? {
        let value = js::get(&object, &name)?;
        if !js::is_nullish(&value)? {
            attrs.insert(name.into(), Value::String(attribute_value(value)?));
        }
    }
    Ok(Some(attrs))
}

#[napi(object)]
pub struct RenderedJs<'env> {
    pub dom: Unknown<'env>,
    #[napi(js_name = "contentDOM")]
    pub content_dom: Option<Unknown<'env>>,
}

fn rendered_js<'env>(env: &'env Env, rendered: Rendered<JsNode>) -> Result<RenderedJs<'env>> {
    Ok(RenderedJs {
        dom: rendered.dom.value(env)?,
        content_dom: rendered.content_dom.map(|dom| dom.value(env)).transpose()?,
    })
}

#[napi]
pub struct DomSerializerHandle {
    serializer: DomSerializer<JsNode>,
}

#[napi]
impl DomSerializerHandle {
    /// A serializer of the `toDOM` functions in `nodes` and `marks`, by type name. A mark type
    /// whose entry isn't a function isn't serialized.
    #[napi(constructor)]
    pub fn new(nodes: Object, marks: Object) -> Result<Self> {
        let mut node_hooks: HashMap<String, NodeToDom<JsNode>> = HashMap::new();
        for name in Object::keys(&nodes)? {
            if let Some(hook) = Hook::method(&nodes, &name)? {
                let to_dom: NodeToDom<JsNode> = Arc::new(move |node| {
                    js::host(|env| spec(hook.call(env, node::wrap(env, node)?)?))
                });
                node_hooks.insert(name, to_dom);
            }
        }
        let mut mark_hooks: HashMap<String, MarkToDom<JsNode>> = HashMap::new();
        for name in Object::keys(&marks)? {
            if let Some(hook) = Hook::of(js::get(&marks, &name)?)? {
                let to_dom: MarkToDom<JsNode> = Arc::new(move |mark, inline| {
                    js::host(|env| {
                        let args = FnArgs::from((mark::wrap(env, mark)?, inline));
                        spec(hook.call(env, args)?)
                    })
                });
                mark_hooks.insert(name, to_dom);
            }
        }
        Ok(DomSerializerHandle {
            serializer: DomSerializer::new(node_hooks, mark_hooks),
        })
    }

    #[napi]
    pub fn serialize_fragment<'env>(
        &self,
        env: &'env Env,
        fragment: &FragmentHandle,
        document: Object,
        target: Option<Unknown>,
    ) -> Result<Unknown<'env>> {
        let dom = JsDom {
            document: Some(document),
        };
        let target = target.map(JsNode::new).transpose()?;
        let result = self
            .serializer
            .serialize_fragment(&dom, &fragment.fragment, target)
            .or_throw(env)?;
        result.value(env)
    }

    #[napi]
    pub fn serialize_node<'env>(
        &self,
        env: &'env Env,
        node: &NodeHandle,
        document: Object,
    ) -> Result<Unknown<'env>> {
        let dom = JsDom {
            document: Some(document),
        };
        let result = self
            .serializer
            .serialize_node(&dom, &node.node)
            .or_throw(env)?;
        result.value(env)
    }

    #[napi]
    pub fn serialize_mark<'env>(
        &self,
        env: &'env Env,
        mark: &MarkHandle,
        inline: bool,
        document: Object,
    ) -> Result<Option<RenderedJs<'env>>> {
        let dom = JsDom {
            document: Some(document),
        };
        let rendered = self
            .serializer
            .serialize_mark(&dom, &mark.mark, inline)
            .or_throw(env)?;
        rendered
            .map(|rendered| rendered_js(env, rendered))
            .transpose()
    }
}

#[napi]
pub fn render_spec<'env>(
    env: &'env Env,
    document: Object,
    structure: Unknown,
    xml_ns: Option<String>,
) -> Result<RenderedJs<'env>> {
    let dom = JsDom {
        document: Some(document),
    };
    let rendered =
        tarnish::dom::render_spec(&dom, &spec(structure)?, xml_ns.as_deref()).or_throw(env)?;
    rendered_js(env, rendered)
}
