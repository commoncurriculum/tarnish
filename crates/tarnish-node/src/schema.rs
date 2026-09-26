//! Schemas and their node and mark types.

use std::sync::Arc;

use napi::bindgen_prelude::{FnArgs, FromNapiValue, JsObjectValue, Object, Unknown, Utf16String};
use napi::{Env, JsValue, Result, ValueType};
use napi_derive::napi;
use tarnish::{
    AttributeSpec, MarkSpec, MarkType, NodeHook, NodeSpec, NodeType, Schema, SchemaSpec, Text,
    Validate, Whitespace,
};

use crate::fragment::FragmentHandle;
use crate::js::{self, Hook, OrThrow};
use crate::mark::{self, MarkHandle};
use crate::node;

fn node_hook<T: 'static>(hook: Hook, read: fn(Unknown) -> Result<T>) -> NodeHook<T> {
    Arc::new(move |node| js::host(|env| read(hook.call(env, node::wrap(env, node)?)?)))
}

fn flag(spec: &Object, key: &str) -> Result<bool> {
    js::get(spec, key)?.coerce_to_bool()
}

fn optional_flag(spec: &Object, key: &str) -> Result<Option<bool>> {
    let value = js::get(spec, key)?;
    match value.get_type()? {
        ValueType::Undefined => Ok(None),
        _ => value.coerce_to_bool().map(Some),
    }
}

fn attributes(spec: &Object) -> Result<Vec<(String, AttributeSpec)>> {
    let attrs = js::get(spec, "attrs")?;
    if attrs.get_type()? != ValueType::Object {
        return Ok(Vec::new());
    }
    let attrs = Object::from_unknown(attrs)?;
    Object::keys(&attrs)?
        .into_iter()
        .map(|name| {
            let attr = Object::from_unknown(js::get(&attrs, &name)?)?;
            let default = match attr.has_own_property("default")? {
                true => Some(js::value_from_js(js::get(&attr, "default")?)?),
                false => None,
            };
            let validate = match js::get_string(&attr, "validate")? {
                Some(types) => Some(Validate::Types(types)),
                None => Hook::method(&attr, "validate")?.map(|validate| {
                    Validate::Hook(Arc::new(move |value| {
                        js::host(|env| {
                            validate.call(env, js::optional_to_js(env, value)?)?;
                            Ok(())
                        })
                    }))
                }),
            };
            Ok((name, AttributeSpec { default, validate }))
        })
        .collect()
}

fn node_spec(spec: &Object) -> Result<NodeSpec> {
    Ok(NodeSpec {
        content: js::get_string(spec, "content")?,
        marks: js::get_string(spec, "marks")?,
        group: js::get_string(spec, "group")?,
        inline: flag(spec, "inline")?,
        atom: flag(spec, "atom")?,
        attrs: attributes(spec)?,
        selectable: optional_flag(spec, "selectable")?,
        draggable: flag(spec, "draggable")?,
        code: flag(spec, "code")?,
        whitespace: match js::get_string(spec, "whitespace")?.as_deref() {
            Some("pre") => Some(Whitespace::Pre),
            Some("normal") => Some(Whitespace::Normal),
            _ => None,
        },
        defining_as_context: flag(spec, "definingAsContext")?,
        defining_for_content: flag(spec, "definingForContent")?,
        defining: flag(spec, "defining")?,
        isolating: flag(spec, "isolating")?,
        linebreak_replacement: flag(spec, "linebreakReplacement")?,
        leaf_text: Hook::method(spec, "leafText")?.map(|hook| node_hook(hook, js::text_from_js)),
        to_debug_string: Hook::method(spec, "toDebugString")?
            .map(|hook| node_hook(hook, String::from_unknown)),
    })
}

fn mark_spec(spec: &Object) -> Result<MarkSpec> {
    let inclusive = js::get(spec, "inclusive")?;
    Ok(MarkSpec {
        attrs: attributes(spec)?,
        inclusive: match inclusive.get_type()? {
            ValueType::Boolean => Some(inclusive.coerce_to_bool()?),
            _ => None,
        },
        excludes: js::get_string(spec, "excludes")?,
        group: js::get_string(spec, "group")?,
        spanning: optional_flag(spec, "spanning")?,
        code: flag(spec, "code")?,
    })
}

fn entries<T>(
    entries: Vec<(String, Object)>,
    read: fn(&Object) -> Result<T>,
) -> Result<Vec<(String, T)>> {
    entries
        .into_iter()
        .map(|(name, spec)| Ok((name, read(&spec)?)))
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
        env: &Env,
        nodes: Vec<(String, Object)>,
        marks: Vec<(String, Object)>,
        top_node: Option<String>,
    ) -> Result<Self> {
        let spec = SchemaSpec {
            nodes: entries(nodes, node_spec)?,
            marks: entries(marks, mark_spec)?,
            top_node,
        };
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
