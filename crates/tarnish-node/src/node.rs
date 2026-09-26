//! Nodes.

use napi::{Env, Result, sys};
use napi_derive::napi;
use tarnish::{ChildAt, Node, Text};

use crate::fragment::{self, FragmentArg};
use crate::js::{self, Data, Js, JsText, OrThrow};
use crate::mark::{self, MarkArg};
use crate::schema::{self, MarkTypeArg, NodeTypeArg};
use crate::slice::{self, SliceArg};

pub struct NodeArg(pub Node);

crate::handle_arg!(NodeArg, NodeHandle, |handle| handle.node.clone());

pub fn wrap(env: sys::napi_env, node: &Node) -> Result<sys::napi_value> {
    let handle = unsafe {
        napi::bindgen_prelude::ToNapiValue::to_napi_value(env, NodeHandle { node: node.clone() })
    }?;
    let id = js::number(env, node.id() as f64)?;
    js::call_registered(env, "wrapNode", &[handle, id])
}

fn wrap_option(env: sys::napi_env, node: Option<&Node>) -> Result<sys::napi_value> {
    match node {
        Some(node) => wrap(env, node),
        None => js::null(env),
    }
}

/// A JavaScript `(node, pos, parent, index)` callback as tarnish calls it: going on into a
/// node's children unless it returns `false`.
pub fn visitor(
    env: sys::napi_env,
    f: sys::napi_value,
) -> impl FnMut(&Node, usize, Option<&Node>, usize) -> tarnish::Result<bool> {
    move |node, pos, parent, index| {
        let result = (|| {
            let args = [
                wrap(env, node)?,
                js::number(env, pos as f64)?,
                wrap_option(env, parent)?,
                js::number(env, index as f64)?,
            ];
            let result = js::call(env, js::undefined(env)?, f, &args)?;
            Ok(!js::is_false(env, result)?)
        })();
        result.map_err(|error| js::host_error(env, error))
    }
}

/// `textBetween`'s `leafText`, a string or a function of the node, as tarnish calls it.
pub fn leaf_text_hook(
    env: sys::napi_env,
    leaf_text: Option<Js>,
) -> Result<Option<impl FnMut(&Node) -> tarnish::Result<Text>>> {
    let Some(Js(leaf_text)) = leaf_text else {
        return Ok(None);
    };
    if !js::truthy(env, leaf_text)? {
        return Ok(None);
    }
    let is_function = js::type_of(env, leaf_text)? == sys::ValueType::napi_function;
    Ok(Some(move |node: &Node| {
        let result = (|| {
            let text = match is_function {
                true => js::call(env, js::undefined(env)?, leaf_text, &[wrap(env, node)?])?,
                false => leaf_text,
            };
            js::text_from_js(env, js::coerce_to_string(env, text)?)
        })();
        result.map_err(|error| js::host_error(env, error))
    }))
}

/// A block separator as `textBetween` takes one: only a string that isn't empty separates.
pub fn separator(env: sys::napi_env, separator: Option<Js>) -> Result<Option<Text>> {
    match separator {
        Some(Js(separator)) if js::type_of(env, separator)? == sys::ValueType::napi_string => {
            Ok(Some(js::text_from_js(env, separator)?))
        }
        _ => Ok(None),
    }
}

#[napi(object)]
pub struct ChildInfo {
    pub node: Option<Js>,
    pub index: u32,
    pub offset: u32,
}

fn child_info(env: sys::napi_env, child: ChildAt) -> Result<ChildInfo> {
    Ok(ChildInfo {
        node: match child.node {
            Some(node) => Some(Js(wrap(env, node)?)),
            None => Some(Js(js::null(env)?)),
        },
        index: child.index as u32,
        offset: child.offset as u32,
    })
}

#[napi]
pub struct NodeHandle {
    pub(crate) node: Node,
}

#[napi]
impl NodeHandle {
    #[napi]
    pub fn node_type(&self, env: Env) -> Result<Js> {
        schema::wrap_node_type(env.raw(), self.node.node_type()).map(Js)
    }

    #[napi]
    pub fn attrs(&self) -> Data {
        Data(tarnish::Value::Object(self.node.attrs().clone()))
    }

    #[napi]
    pub fn content(&self, env: Env) -> Result<Js> {
        fragment::wrap(env.raw(), self.node.content()).map(Js)
    }

    #[napi]
    pub fn marks(&self, env: Env) -> Result<Js> {
        mark::wrap_set(env.raw(), self.node.marks()).map(Js)
    }

    #[napi]
    pub fn text(&self) -> Option<JsText> {
        self.node.text().cloned().map(JsText)
    }

    #[napi(getter)]
    pub fn is_text(&self) -> bool {
        self.node.is_text()
    }

    #[napi(getter)]
    pub fn node_size(&self) -> u32 {
        self.node.node_size() as u32
    }

    #[napi]
    pub fn nodes_between(&self, env: Env, from: u32, to: u32, f: Js, start_pos: u32) -> Result<()> {
        let mut visit = visitor(env.raw(), f.0);
        self.node
            .nodes_between(from as usize, to as usize, &mut visit, start_pos as usize)
            .or_throw(&env)
    }

    #[napi]
    pub fn text_content(&self, env: Env) -> Result<JsText> {
        self.node.text_content().map(JsText).or_throw(&env)
    }

    #[napi]
    pub fn text_between(
        &self,
        env: Env,
        from: u32,
        to: u32,
        block_separator: Option<Js>,
        leaf_text: Option<Js>,
    ) -> Result<JsText> {
        let separator = separator(env.raw(), block_separator)?;
        let mut leaf_text = leaf_text_hook(env.raw(), leaf_text)?;
        let leaf_text = leaf_text.as_mut().map(|f| f as &mut tarnish::LeafTextHook);
        self.node
            .text_between(from as usize, to as usize, separator.as_ref(), leaf_text)
            .map(JsText)
            .or_throw(&env)
    }

    #[napi]
    pub fn eq(&self, other: NodeArg) -> bool {
        self.node == other.0
    }

    #[napi]
    pub fn same_markup(&self, other: NodeArg) -> bool {
        self.node.same_markup(&other.0)
    }

    #[napi]
    pub fn has_markup(
        &self,
        node_type: NodeTypeArg,
        attrs: Option<Data>,
        marks: Option<Vec<MarkArg>>,
    ) -> bool {
        let attrs = attrs.map(|attrs| attrs.0);
        let attrs = attrs.as_ref().and_then(|attrs| attrs.as_attrs());
        let marks = marks.map(|marks| mark::list(Some(marks)));
        self.node.has_markup(&node_type.0, attrs, marks.as_deref())
    }

    #[napi]
    pub fn copy(&self, env: Env, content: FragmentArg) -> Result<Js> {
        wrap(env.raw(), &self.node.copy(content.0)).map(Js)
    }

    #[napi]
    pub fn mark(&self, env: Env, marks: Vec<MarkArg>) -> Result<Js> {
        wrap(env.raw(), &self.node.mark(mark::list(Some(marks)).into())).map(Js)
    }

    #[napi]
    pub fn with_text(&self, env: Env, text: JsText) -> Result<Js> {
        let node = self.node.with_text(text.0).or_throw(&env)?;
        wrap(env.raw(), &node).map(Js)
    }

    #[napi]
    pub fn cut(&self, env: Env, from: u32, to: u32) -> Result<Js> {
        let node = self.node.cut(from as usize, to as usize).or_throw(&env)?;
        wrap(env.raw(), &node).map(Js)
    }

    #[napi]
    pub fn slice(&self, env: Env, from: u32, to: u32, include_parents: bool) -> Result<Js> {
        let slice = self
            .node
            .slice(from as usize, to as usize, include_parents)
            .or_throw(&env)?;
        slice::wrap(env.raw(), &slice).map(Js)
    }

    #[napi]
    pub fn replace(&self, env: Env, from: u32, to: u32, slice: SliceArg) -> Result<Js> {
        let node = self
            .node
            .replace(from as usize, to as usize, &slice.0)
            .or_throw(&env)?;
        wrap(env.raw(), &node).map(Js)
    }

    #[napi]
    pub fn node_at(&self, env: Env, pos: u32) -> Result<Js> {
        let node = self.node.node_at(pos as usize).or_throw(&env)?;
        wrap_option(env.raw(), node).map(Js)
    }

    #[napi]
    pub fn child_after(&self, env: Env, pos: u32) -> Result<ChildInfo> {
        let child = self.node.child_after(pos as usize).or_throw(&env)?;
        child_info(env.raw(), child)
    }

    #[napi]
    pub fn child_before(&self, env: Env, pos: u32) -> Result<ChildInfo> {
        let child = self.node.child_before(pos as usize).or_throw(&env)?;
        child_info(env.raw(), child)
    }

    #[napi]
    pub fn resolve(&self, env: Env, pos: f64) -> Result<Js> {
        let resolved = self
            .node
            .resolve(crate::position::pos_arg(pos))
            .or_throw(&env)?;
        crate::position::wrap(env.raw(), resolved).map(Js)
    }

    #[napi]
    pub fn range_has_mark(&self, env: Env, from: u32, to: u32, mark: MarkArg) -> Result<bool> {
        self.node
            .range_has_mark(from as usize, to as usize, &mark.0)
            .or_throw(&env)
    }

    #[napi]
    pub fn range_has_mark_type(
        &self,
        env: Env,
        from: u32,
        to: u32,
        mark_type: MarkTypeArg,
    ) -> Result<bool> {
        self.node
            .range_has_mark_type(from as usize, to as usize, &mark_type.0)
            .or_throw(&env)
    }

    #[napi]
    pub fn to_debug_string(&self, env: Env) -> Result<String> {
        self.node.to_debug_string().or_throw(&env)
    }

    #[napi]
    pub fn content_match_at(&self, env: Env, index: u32) -> Result<Js> {
        let found = self.node.content_match_at(index as usize).or_throw(&env)?;
        crate::content::wrap(env.raw(), &found).map(Js)
    }

    #[napi]
    pub fn can_replace(
        &self,
        env: Env,
        from: u32,
        to: u32,
        replacement: FragmentArg,
        start: u32,
        end: u32,
    ) -> Result<bool> {
        self.node
            .can_replace(
                from as usize,
                to as usize,
                &replacement.0,
                start as usize,
                end as usize,
            )
            .or_throw(&env)
    }

    #[napi]
    pub fn can_replace_with(
        &self,
        env: Env,
        from: u32,
        to: u32,
        node_type: NodeTypeArg,
        marks: Option<Vec<MarkArg>>,
    ) -> Result<bool> {
        let marks = marks.map(|marks| mark::list(Some(marks)));
        self.node
            .can_replace_with(from as usize, to as usize, &node_type.0, marks.as_deref())
            .or_throw(&env)
    }

    #[napi]
    pub fn can_append(&self, env: Env, other: NodeArg) -> Result<bool> {
        self.node.can_append(&other.0).or_throw(&env)
    }

    #[napi]
    pub fn check(&self, env: Env) -> Result<()> {
        self.node.check().or_throw(&env)
    }

    #[napi]
    pub fn to_json(&self) -> Data {
        Data(self.node.to_json())
    }
}
