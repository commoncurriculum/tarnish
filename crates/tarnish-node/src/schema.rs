//! Schemas and their node and mark types.

use std::sync::Arc;

use napi::{Env, Result, sys};
use napi_derive::napi;
use tarnish::{
    AttributeSpec, MarkSpec, MarkType, NodeHook, NodeSpec, NodeType, Schema, SchemaSpec, Validate,
    Whitespace,
};

use crate::fragment::FragmentArg;
use crate::js::{self, Data, Hook, Js, OrThrow};
use crate::mark::{self, MarkArg, MarkSetArg};
use crate::node;

fn hook(env: sys::napi_env, spec: sys::napi_value, key: &str) -> Result<Option<Arc<Hook>>> {
    Hook::method(env, spec, key)
}

fn node_hook<T: 'static>(
    hook: Option<Arc<Hook>>,
    convert: fn(sys::napi_env, sys::napi_value) -> Result<T>,
) -> Option<NodeHook<T>> {
    let hook = hook?;
    Some(Arc::new(move |node| {
        let result = hook.call(|env| Ok(vec![node::wrap(env, node)?]))?;
        hook.read(|env| convert(env, result))
    }))
}

fn flag(env: sys::napi_env, spec: sys::napi_value, key: &str) -> Result<bool> {
    js::truthy(env, js::get(env, spec, key)?)
}

fn optional_flag(env: sys::napi_env, spec: sys::napi_value, key: &str) -> Result<Option<bool>> {
    let value = js::get(env, spec, key)?;
    match js::type_of(env, value)? {
        sys::ValueType::napi_undefined => Ok(None),
        _ => Ok(Some(js::truthy(env, value)?)),
    }
}

fn attributes(env: sys::napi_env, spec: sys::napi_value) -> Result<Vec<(String, AttributeSpec)>> {
    let attrs = js::get(env, spec, "attrs")?;
    if js::type_of(env, attrs)? != sys::ValueType::napi_object {
        return Ok(Vec::new());
    }
    js::property_names(env, attrs)?
        .into_iter()
        .map(|name| {
            let attr = js::get(env, attrs, &name)?;
            let default = match js::has_own(env, attr, "default")? {
                true => Some(js::value_from_js(env, js::get(env, attr, "default")?)?),
                false => None,
            };
            let validate = match js::get_string(env, attr, "validate")? {
                Some(types) => Some(Validate::Types(types)),
                None => hook(env, attr, "validate")?.map(|hook| {
                    Validate::Hook(Arc::new(move |value: &tarnish::Value| {
                        hook.call(|env| Ok(vec![js::value_to_js(env, value)?]))
                            .map(|_| ())
                    }))
                }),
            };
            Ok((name, AttributeSpec { default, validate }))
        })
        .collect()
}

fn node_spec(env: sys::napi_env, spec: sys::napi_value) -> Result<NodeSpec> {
    Ok(NodeSpec {
        content: js::get_string(env, spec, "content")?,
        marks: js::get_string(env, spec, "marks")?,
        group: js::get_string(env, spec, "group")?,
        inline: flag(env, spec, "inline")?,
        atom: flag(env, spec, "atom")?,
        attrs: attributes(env, spec)?,
        selectable: optional_flag(env, spec, "selectable")?,
        draggable: flag(env, spec, "draggable")?,
        code: flag(env, spec, "code")?,
        whitespace: match js::get_string(env, spec, "whitespace")?.as_deref() {
            Some("pre") => Some(Whitespace::Pre),
            Some("normal") => Some(Whitespace::Normal),
            _ => None,
        },
        defining_as_context: flag(env, spec, "definingAsContext")?,
        defining_for_content: flag(env, spec, "definingForContent")?,
        defining: flag(env, spec, "defining")?,
        isolating: flag(env, spec, "isolating")?,
        linebreak_replacement: flag(env, spec, "linebreakReplacement")?,
        leaf_text: node_hook(hook(env, spec, "leafText")?, js::text_from_js),
        to_debug_string: node_hook(hook(env, spec, "toDebugString")?, |env, value| unsafe {
            <String as napi::bindgen_prelude::FromNapiValue>::from_napi_value(env, value)
        }),
    })
}

fn mark_spec(env: sys::napi_env, spec: sys::napi_value) -> Result<MarkSpec> {
    let inclusive = js::get(env, spec, "inclusive")?;
    Ok(MarkSpec {
        attrs: attributes(env, spec)?,
        inclusive: match js::type_of(env, inclusive)? {
            sys::ValueType::napi_boolean => Some(js::truthy(env, inclusive)?),
            _ => None,
        },
        excludes: js::get_string(env, spec, "excludes")?,
        group: js::get_string(env, spec, "group")?,
        spanning: optional_flag(env, spec, "spanning")?,
        code: flag(env, spec, "code")?,
    })
}

fn entries<T>(
    env: sys::napi_env,
    entries: Vec<(String, Js)>,
    read: fn(sys::napi_env, sys::napi_value) -> Result<T>,
) -> Result<Vec<(String, T)>> {
    entries
        .into_iter()
        .map(|(name, spec)| Ok((name, read(env, spec.0)?)))
        .collect()
}

#[napi]
pub struct SchemaHandle {
    pub(crate) schema: Schema,
}

#[napi]
impl SchemaHandle {
    /// A schema from its node and mark specs, each a name and its spec object, in order.
    #[napi(constructor)]
    pub fn new(
        env: Env,
        nodes: Vec<(String, Js)>,
        marks: Vec<(String, Js)>,
        top_node: Option<String>,
    ) -> Result<Self> {
        let spec = SchemaSpec {
            nodes: entries(env.raw(), nodes, node_spec)?,
            marks: entries(env.raw(), marks, mark_spec)?,
            top_node,
        };
        Ok(SchemaHandle {
            schema: Schema::new(spec).or_throw(&env)?,
        })
    }

    #[napi(getter)]
    pub fn id(&self) -> f64 {
        schema_id(&self.schema)
    }

    #[napi]
    pub fn node_type(&self, index: u32) -> NodeTypeHandle {
        NodeTypeHandle {
            node_type: self
                .schema
                .node_types()
                .nth(index as usize)
                .expect("an index of the schema's"),
        }
    }

    #[napi]
    pub fn mark_type(&self, index: u32) -> MarkTypeHandle {
        MarkTypeHandle {
            mark_type: self
                .schema
                .mark_types()
                .nth(index as usize)
                .expect("an index of the schema's"),
        }
    }

    #[napi(getter)]
    pub fn top_node_type(&self) -> u32 {
        self.schema.top_node_type().index() as u32
    }

    #[napi(getter)]
    pub fn linebreak_replacement(&self) -> Option<u32> {
        self.schema
            .linebreak_replacement()
            .map(|node_type| node_type.index() as u32)
    }

    #[napi]
    pub fn text(&self, env: Env, text: js::JsText, marks: Option<Vec<MarkArg>>) -> Result<Js> {
        let node = self
            .schema
            .text(text.0, &mark::list(marks))
            .or_throw(&env)?;
        node::wrap(env.raw(), &node).map(Js)
    }

    #[napi]
    pub fn node_from_json(&self, env: Env, json: Data) -> Result<Js> {
        let node = tarnish::Node::from_json(&self.schema, &json.0).or_throw(&env)?;
        node::wrap(env.raw(), &node).map(Js)
    }

    #[napi]
    pub fn mark_from_json(&self, env: Env, json: Data) -> Result<Js> {
        let mark = tarnish::Mark::from_json(&self.schema, &json.0).or_throw(&env)?;
        mark::wrap(env.raw(), &mark).map(Js)
    }

    /// The index of the node type of this name, raising an error when there is none.
    #[napi]
    pub fn expect_node_type(&self, env: Env, name: String) -> Result<u32> {
        let node_type = self.schema.expect_node_type(&name).or_throw(&env)?;
        Ok(node_type.index() as u32)
    }
}

pub fn schema_id(schema: &Schema) -> f64 {
    schema.id() as f64
}

/// The wrapper of the node type, from the schema's.
pub fn wrap_node_type(env: sys::napi_env, node_type: &NodeType) -> Result<sys::napi_value> {
    let (schema, index) = (
        js::number(env, schema_id(node_type.schema()))?,
        js::number(env, node_type.index() as f64)?,
    );
    js::call_registered(env, "wrapNodeType", &[schema, index])
}

pub fn wrap_mark_type(env: sys::napi_env, mark_type: &MarkType) -> Result<sys::napi_value> {
    let (schema, index) = (
        js::number(env, schema_id(mark_type.schema()))?,
        js::number(env, mark_type.rank() as f64)?,
    );
    js::call_registered(env, "wrapMarkType", &[schema, index])
}

/// A node type wrapper given to the bridge, read through its handle.
pub struct NodeTypeArg(pub NodeType);

crate::handle_arg!(NodeTypeArg, NodeTypeHandle, |handle| handle
    .node_type
    .clone());

#[napi]
pub struct NodeTypeHandle {
    pub(crate) node_type: NodeType,
}

fn attrs_arg(attrs: Option<Data>) -> Option<tarnish::Value> {
    attrs.map(|attrs| attrs.0)
}

#[napi]
impl NodeTypeHandle {
    #[napi(getter)]
    pub fn name(&self) -> String {
        self.node_type.name().to_owned()
    }

    #[napi(getter)]
    pub fn is_block(&self) -> bool {
        self.node_type.is_block()
    }

    #[napi(getter)]
    pub fn is_text(&self) -> bool {
        self.node_type.is_text()
    }

    #[napi(getter)]
    pub fn inline_content(&self) -> bool {
        self.node_type.inline_content()
    }

    #[napi(getter)]
    pub fn is_leaf(&self) -> bool {
        self.node_type.is_leaf()
    }

    #[napi(getter)]
    pub fn groups(&self) -> Vec<String> {
        self.node_type.groups().to_vec()
    }

    #[napi(getter)]
    pub fn whitespace(&self) -> &'static str {
        match self.node_type.whitespace() {
            Whitespace::Pre => "pre",
            Whitespace::Normal => "normal",
        }
    }

    #[napi(getter)]
    pub fn has_required_attrs(&self) -> bool {
        self.node_type.has_required_attrs()
    }

    #[napi(getter)]
    pub fn default_attrs(&self) -> Option<Data> {
        let defaults = self.node_type.default_attrs()?;
        Some(Data(tarnish::Value::Object(defaults.clone())))
    }

    /// The indexes of the marks allowed in the type's nodes, `null` for all.
    #[napi(getter)]
    pub fn mark_set(&self) -> Option<Vec<u32>> {
        let marks = self.node_type.mark_set()?;
        Some(marks.iter().map(|mark| mark.rank() as u32).collect())
    }

    #[napi]
    pub fn content_match(&self, env: Env) -> Result<Js> {
        crate::content::wrap(env.raw(), &self.node_type.content_match()).map(Js)
    }

    #[napi]
    pub fn compatible_content(&self, other: NodeTypeArg) -> bool {
        self.node_type.compatible_content(&other.0)
    }

    #[napi]
    pub fn compute_attrs(&self, env: Env, attrs: Option<Data>) -> Result<Data> {
        let attrs = attrs_arg(attrs);
        let computed = self
            .node_type
            .compute_attrs(attrs.as_ref().and_then(|a| a.as_attrs()))
            .or_throw(&env)?;
        Ok(Data(tarnish::Value::Object(computed)))
    }

    #[napi]
    pub fn create(
        &self,
        env: Env,
        attrs: Option<Data>,
        content: FragmentArg,
        marks: Option<Vec<MarkArg>>,
    ) -> Result<Js> {
        let attrs = attrs_arg(attrs);
        let attrs = attrs.as_ref().and_then(|a| a.as_attrs());
        let node = self
            .node_type
            .create(attrs, content.0, &mark::list(marks))
            .or_throw(&env)?;
        node::wrap(env.raw(), &node).map(Js)
    }

    #[napi]
    pub fn create_checked(
        &self,
        env: Env,
        attrs: Option<Data>,
        content: FragmentArg,
        marks: Option<Vec<MarkArg>>,
    ) -> Result<Js> {
        let attrs = attrs_arg(attrs);
        let attrs = attrs.as_ref().and_then(|a| a.as_attrs());
        let node = self
            .node_type
            .create_checked(attrs, content.0, &mark::list(marks))
            .or_throw(&env)?;
        node::wrap(env.raw(), &node).map(Js)
    }

    #[napi]
    pub fn create_and_fill(
        &self,
        env: Env,
        attrs: Option<Data>,
        content: FragmentArg,
        marks: Option<Vec<MarkArg>>,
    ) -> Result<Option<Js>> {
        let attrs = attrs_arg(attrs);
        let attrs = attrs.as_ref().and_then(|a| a.as_attrs());
        let node = self
            .node_type
            .create_and_fill(attrs, content.0, &mark::list(marks))
            .or_throw(&env)?;
        node.map(|node| node::wrap(env.raw(), &node).map(Js))
            .transpose()
    }

    #[napi]
    pub fn valid_content(&self, content: FragmentArg) -> bool {
        self.node_type.valid_content(&content.0)
    }

    #[napi]
    pub fn check_content(&self, env: Env, content: FragmentArg) -> Result<()> {
        self.node_type.check_content(&content.0).or_throw(&env)
    }

    #[napi]
    pub fn check_attrs(&self, env: Env, attrs: Data) -> Result<()> {
        let attrs = attrs.0.as_attrs().cloned().unwrap_or_default();
        self.node_type.check_attrs(&attrs).or_throw(&env)
    }

    #[napi]
    pub fn allows_mark_type(&self, mark_type: MarkTypeArg) -> bool {
        self.node_type.allows_mark_type(&mark_type.0)
    }

    #[napi]
    pub fn allows_marks(&self, marks: Vec<MarkArg>) -> bool {
        self.node_type.allows_marks(&mark::list(Some(marks)))
    }

    #[napi]
    pub fn allowed_marks(&self, env: Env, marks: MarkSetArg) -> Result<Js> {
        marks.give_back(env.raw(), &self.node_type.allowed_marks(marks.marks()))
    }
}

/// A mark type wrapper given to the bridge, read through its handle.
pub struct MarkTypeArg(pub MarkType);

crate::handle_arg!(MarkTypeArg, MarkTypeHandle, |handle| handle
    .mark_type
    .clone());

#[napi]
pub struct MarkTypeHandle {
    pub(crate) mark_type: MarkType,
}

#[napi]
impl MarkTypeHandle {
    #[napi(getter)]
    pub fn name(&self) -> String {
        self.mark_type.name().to_owned()
    }

    #[napi(getter)]
    pub fn rank(&self) -> u32 {
        self.mark_type.rank() as u32
    }

    #[napi(getter)]
    pub fn default_attrs(&self) -> Option<Data> {
        let defaults = self.mark_type.default_attrs()?;
        Some(Data(tarnish::Value::Object(defaults.clone())))
    }

    /// The indexes of the mark types this one excludes.
    #[napi(getter)]
    pub fn excluded(&self) -> Vec<u32> {
        self.mark_type
            .excluded()
            .iter()
            .map(|mark| mark.rank() as u32)
            .collect()
    }

    #[napi]
    pub fn create(&self, env: Env, attrs: Option<Data>) -> Result<Js> {
        let attrs = attrs_arg(attrs);
        let mark = self
            .mark_type
            .create(attrs.as_ref().and_then(|a| a.as_attrs()))
            .or_throw(&env)?;
        mark::wrap(env.raw(), &mark).map(Js)
    }

    #[napi]
    pub fn remove_from_set(&self, env: Env, set: MarkSetArg) -> Result<Js> {
        set.give_back(env.raw(), &self.mark_type.remove_from_set(set.marks()))
    }

    #[napi]
    pub fn is_in_set(&self, env: Env, set: Vec<MarkArg>) -> Result<Option<Js>> {
        let set = mark::list(Some(set));
        self.mark_type
            .is_in_set(&set)
            .map(|mark| mark::wrap(env.raw(), mark).map(Js))
            .transpose()
    }

    #[napi]
    pub fn check_attrs(&self, env: Env, attrs: Data) -> Result<()> {
        let attrs = attrs.0.as_attrs().cloned().unwrap_or_default();
        self.mark_type.check_attrs(&attrs).or_throw(&env)
    }

    #[napi]
    pub fn excludes(&self, other: MarkTypeArg) -> bool {
        self.mark_type.excludes(&other.0)
    }
}
