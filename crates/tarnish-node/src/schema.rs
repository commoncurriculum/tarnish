//! Schemas and their node and mark types.

use std::sync::Arc;

use napi::bindgen_prelude::{FnArgs, FromNapiValue, JsObjectValue, Object, Unknown, Utf16String};
use napi::{Env, Error, Result};
use napi_derive::napi;
use tarnish::{
    AttributeSpec, MarkType, NodeHook, NodeType, Schema, SchemaSpec, Text, Validate, Whitespace,
};

use crate::fragment::FragmentHandle;
use crate::js::{self, Hook, OrThrow};
use crate::mark::{self, MarkHandle};
use crate::node;

fn node_hook<T: 'static>(hook: Hook, read: fn(Unknown) -> Result<T>) -> NodeHook<T> {
    Arc::new(move |node| js::host(|env| read(hook.call(env, node::wrap(env, node)?)?)))
}

/// The functions of attribute specs, and defaults of `undefined`, which JSON leaves out.
fn attribute_hooks(attrs: &mut [(String, AttributeSpec)], spec: &Object) -> Result<()> {
    if attrs.is_empty() {
        return Ok(());
    }
    let specs = Object::from_unknown(js::get(spec, "attrs")?)?;
    for (name, attr) in attrs {
        let spec = Object::from_unknown(js::get(&specs, name)?)?;
        if let Some(validate) = Hook::method(&spec, "validate")? {
            attr.validate = Some(Validate::Hook(Arc::new(move |value| {
                js::host(|env| {
                    validate.call(env, js::optional_to_js(env, value)?)?;
                    Ok(())
                })
            })));
        }
        if attr.default.is_none() && spec.has_own_property("default")? {
            attr.default = Some(None);
        }
    }
    Ok(())
}

#[napi]
pub struct SchemaHandle {
    pub(crate) schema: Schema,
}

#[napi]
impl SchemaHandle {
    /// A schema from its spec's JSON, whose `nodes` and `marks` are `[name, spec]` pairs, and
    /// the node and mark specs themselves, in the same order, for their functions.
    #[napi(constructor)]
    pub fn new(env: &Env, json: String, nodes: Vec<Object>, marks: Vec<Object>) -> Result<Self> {
        let json = tarnish::json::from_str(&json)
            .map_err(|_| Error::from_reason("A schema spec's JSON didn't parse"))?;
        let mut spec = SchemaSpec::from_json(&json).or_throw(env)?;
        for ((_, spec), object) in spec.nodes.iter_mut().zip(&nodes) {
            spec.leaf_text =
                Hook::method(object, "leafText")?.map(|hook| node_hook(hook, js::text_from_js));
            spec.to_debug_string = Hook::method(object, "toDebugString")?
                .map(|hook| node_hook(hook, String::from_unknown));
            attribute_hooks(&mut spec.attrs, object)?;
        }
        for ((_, spec), object) in spec.marks.iter_mut().zip(&marks) {
            attribute_hooks(&mut spec.attrs, object)?;
        }
        Ok(SchemaHandle {
            schema: Schema::new(spec).or_throw(env)?,
        })
    }

    #[napi(getter)]
    pub fn id(&self) -> f64 {
        self.schema.id() as f64
    }

    #[napi]
    pub fn node_type(&self, index: u32) -> Option<NodeTypeHandle> {
        let node_type = self.schema.node_types().nth(index as usize)?;
        Some(NodeTypeHandle { node_type })
    }

    #[napi]
    pub fn mark_type(&self, index: u32) -> Option<MarkTypeHandle> {
        let mark_type = self.schema.mark_types().nth(index as usize)?;
        Some(MarkTypeHandle { mark_type })
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
    pub fn text<'env>(
        &self,
        env: &'env Env,
        text: Utf16String,
        marks: Option<Vec<&MarkHandle>>,
    ) -> Result<Unknown<'env>> {
        let marks = mark::list(marks.unwrap_or_default());
        let node = self
            .schema
            .text(Text::from_units(&text), &marks)
            .or_throw(env)?;
        node::wrap(env, &node)
    }

    #[napi]
    pub fn node_from_json<'env>(&self, env: &'env Env, json: Unknown) -> Result<Unknown<'env>> {
        let node =
            tarnish::Node::from_json(&self.schema, &js::json_from_js(json)?).or_throw(env)?;
        node::wrap(env, &node)
    }

    #[napi]
    pub fn mark_from_json<'env>(&self, env: &'env Env, json: Unknown) -> Result<Unknown<'env>> {
        let mark =
            tarnish::Mark::from_json(&self.schema, &js::json_from_js(json)?).or_throw(env)?;
        mark::wrap(env, &mark)
    }

    /// The index of the node type of this name, raising an error when there is none.
    #[napi]
    pub fn expect_node_type(&self, env: &Env, name: String) -> Result<u32> {
        let node_type = self.schema.expect_node_type(&name).or_throw(env)?;
        Ok(node_type.index() as u32)
    }
}

/// The wrapper of the node type, from the schema's.
pub fn wrap_node_type<'env>(env: &'env Env, node_type: &NodeType) -> Result<Unknown<'env>> {
    let args = (node_type.schema().id() as f64, node_type.index() as f64);
    js::call_registered(env, "wrapNodeType", FnArgs::from(args))
}

pub fn wrap_mark_type<'env>(env: &'env Env, mark_type: &MarkType) -> Result<Unknown<'env>> {
    let args = (mark_type.schema().id() as f64, mark_type.rank() as f64);
    js::call_registered(env, "wrapMarkType", FnArgs::from(args))
}

#[napi]
pub struct NodeTypeHandle {
    pub(crate) node_type: NodeType,
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
    pub fn default_attrs<'env>(&self, env: &'env Env) -> Result<Option<Unknown<'env>>> {
        self.node_type
            .default_attrs()
            .map(|defaults| js::attrs_to_js(env, defaults))
            .transpose()
    }

    /// The indexes of the marks allowed in the type's nodes, `null` for all.
    #[napi(getter)]
    pub fn mark_set(&self) -> Option<Vec<u32>> {
        let marks = self.node_type.mark_set()?;
        Some(marks.iter().map(|mark| mark.rank() as u32).collect())
    }

    #[napi]
    pub fn content_match<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        crate::content::wrap(env, &self.node_type.content_match())
    }

    #[napi]
    pub fn compatible_content(&self, other: &NodeTypeHandle) -> bool {
        self.node_type.compatible_content(&other.node_type)
    }

    #[napi]
    pub fn compute_attrs<'env>(&self, env: &'env Env, attrs: Unknown) -> Result<Unknown<'env>> {
        let attrs = js::attrs_from_js(attrs)?;
        let computed = self
            .node_type
            .compute_attrs(attrs.as_deref())
            .or_throw(env)?;
        js::attrs_to_js(env, &computed)
    }

    #[napi]
    pub fn create<'env>(
        &self,
        env: &'env Env,
        attrs: Unknown,
        content: &FragmentHandle,
        marks: Option<Vec<&MarkHandle>>,
    ) -> Result<Unknown<'env>> {
        let attrs = js::attrs_from_js(attrs)?;
        let marks = mark::list(marks.unwrap_or_default());
        let node = self
            .node_type
            .create(attrs.as_deref(), content.fragment.clone(), &marks)
            .or_throw(env)?;
        node::wrap(env, &node)
    }

    #[napi]
    pub fn create_checked<'env>(
        &self,
        env: &'env Env,
        attrs: Unknown,
        content: &FragmentHandle,
        marks: Option<Vec<&MarkHandle>>,
    ) -> Result<Unknown<'env>> {
        let attrs = js::attrs_from_js(attrs)?;
        let marks = mark::list(marks.unwrap_or_default());
        let node = self
            .node_type
            .create_checked(attrs.as_deref(), content.fragment.clone(), &marks)
            .or_throw(env)?;
        node::wrap(env, &node)
    }

    #[napi]
    pub fn create_and_fill<'env>(
        &self,
        env: &'env Env,
        attrs: Unknown,
        content: &FragmentHandle,
        marks: Option<Vec<&MarkHandle>>,
    ) -> Result<Option<Unknown<'env>>> {
        let attrs = js::attrs_from_js(attrs)?;
        let marks = mark::list(marks.unwrap_or_default());
        let node = self
            .node_type
            .create_and_fill(attrs.as_deref(), content.fragment.clone(), &marks)
            .or_throw(env)?;
        node.map(|node| node::wrap(env, &node)).transpose()
    }

    #[napi]
    pub fn valid_content(&self, content: &FragmentHandle) -> bool {
        self.node_type.valid_content(&content.fragment)
    }

    #[napi]
    pub fn check_content(&self, env: &Env, content: &FragmentHandle) -> Result<()> {
        self.node_type
            .check_content(&content.fragment)
            .or_throw(env)
    }

    #[napi]
    pub fn check_attrs(&self, env: &Env, attrs: Unknown) -> Result<()> {
        let attrs = js::attrs_from_js(attrs)?.unwrap_or_default();
        self.node_type.check_attrs(&attrs).or_throw(env)
    }

    #[napi]
    pub fn allows_mark_type(&self, mark_type: &MarkTypeHandle) -> bool {
        self.node_type.allows_mark_type(&mark_type.mark_type)
    }

    #[napi]
    pub fn allows_marks(&self, marks: Vec<&MarkHandle>) -> bool {
        self.node_type.allows_marks(&mark::list(marks))
    }

    #[napi]
    pub fn allowed_marks<'env>(
        &self,
        env: &'env Env,
        marks: Vec<&MarkHandle>,
    ) -> Result<Option<Unknown<'env>>> {
        let marks = mark::list(marks).into();
        mark::changed_set(env, &marks, &self.node_type.allowed_marks(&marks))
    }
}

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
    pub fn default_attrs<'env>(&self, env: &'env Env) -> Result<Option<Unknown<'env>>> {
        self.mark_type
            .default_attrs()
            .map(|defaults| js::attrs_to_js(env, defaults))
            .transpose()
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
    pub fn create<'env>(&self, env: &'env Env, attrs: Unknown) -> Result<Unknown<'env>> {
        let attrs = js::attrs_from_js(attrs)?;
        let mark = self.mark_type.create(attrs.as_deref()).or_throw(env)?;
        mark::wrap(env, &mark)
    }

    #[napi]
    pub fn remove_from_set<'env>(
        &self,
        env: &'env Env,
        set: Vec<&MarkHandle>,
    ) -> Result<Option<Unknown<'env>>> {
        let set = mark::list(set).into();
        mark::changed_set(env, &set, &self.mark_type.remove_from_set(&set))
    }

    #[napi]
    pub fn is_in_set<'env>(
        &self,
        env: &'env Env,
        set: Vec<&MarkHandle>,
    ) -> Result<Option<Unknown<'env>>> {
        let set = mark::list(set);
        self.mark_type
            .is_in_set(&set)
            .map(|mark| mark::wrap(env, mark))
            .transpose()
    }

    #[napi]
    pub fn check_attrs(&self, env: &Env, attrs: Unknown) -> Result<()> {
        let attrs = js::attrs_from_js(attrs)?.unwrap_or_default();
        self.mark_type.check_attrs(&attrs).or_throw(env)
    }

    #[napi]
    pub fn excludes(&self, other: &MarkTypeHandle) -> bool {
        self.mark_type.excludes(&other.mark_type)
    }
}
