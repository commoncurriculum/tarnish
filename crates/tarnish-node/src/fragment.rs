//! Fragments.

use napi::bindgen_prelude::{FnArgs, Function, Unknown};
use napi::{Env, JsString, JsValue, Result};
use napi_derive::napi;
use tarnish::Fragment;

use crate::js::{self, OrThrow};
use crate::node::{self, NodeHandle};
use crate::schema::SchemaHandle;

pub fn wrap<'env>(env: &'env Env, fragment: &Fragment) -> Result<Unknown<'env>> {
    let handle = FragmentHandle {
        fragment: fragment.clone(),
    };
    js::call_registered(
        env,
        "wrapFragment",
        FnArgs::from((handle, fragment.id() as f64)),
    )
}

#[napi(object)]
pub struct DiffEnd {
    pub a: u32,
    pub b: u32,
}

#[napi(object)]
pub struct IndexInfo {
    pub index: u32,
    pub offset: u32,
}

#[napi]
pub struct FragmentHandle {
    pub(crate) fragment: Fragment,
}

#[napi]
impl FragmentHandle {
    #[napi(getter)]
    pub fn size(&self) -> u32 {
        self.fragment.size() as u32
    }

    #[napi(getter)]
    pub fn child_count(&self) -> u32 {
        self.fragment.child_count() as u32
    }

    #[napi]
    pub fn children<'env>(&self, env: &'env Env) -> Result<Vec<Unknown<'env>>> {
        self.fragment
            .children()
            .iter()
            .map(|child| node::wrap(env, child))
            .collect()
    }

    #[napi]
    pub fn child<'env>(&self, env: &'env Env, index: u32) -> Result<Unknown<'env>> {
        let child = self.fragment.child(index as usize).or_throw(env)?;
        node::wrap(env, child)
    }

    #[napi]
    pub fn nodes_between(
        &self,
        env: &Env,
        from: f64,
        to: f64,
        f: Function,
        node_start: f64,
        parent: Option<&NodeHandle>,
    ) -> Result<()> {
        let (from, to, node_start) = (
            js::pos(env, from)?,
            js::pos(env, to)?,
            js::pos(env, node_start)?,
        );
        let parent = parent.map(|parent| &parent.node);
        self.fragment
            .nodes_between(from, to, &mut node::visitor(f), node_start, parent)
            .or_throw(env)
    }

    #[napi]
    pub fn text_between<'env>(
        &self,
        env: &'env Env,
        from: f64,
        to: f64,
        block_separator: Option<Unknown>,
        leaf_text: Option<Unknown>,
    ) -> Result<JsString<'env>> {
        let (from, to) = (js::pos(env, from)?, js::pos(env, to)?);
        let separator = node::separator(block_separator)?;
        let mut leaf_text = node::leaf_text(leaf_text)?;
        let leaf_text = leaf_text.as_mut().map(|f| f as &mut tarnish::LeafTextHook);
        let text = self
            .fragment
            .text_between(from, to, separator.as_ref(), leaf_text)
            .or_throw(env)?;
        js::text_to_js(env, &text)
    }

    #[napi]
    pub fn for_each(&self, env: &Env, f: Function) -> Result<()> {
        for (index, (offset, child)) in self.fragment.children_with_offsets().enumerate() {
            let args = (node::wrap(env, child)?, offset as f64, index as f64);
            js::call(f.to_unknown(), FnArgs::from(args))?;
        }
        Ok(())
    }

    #[napi]
    pub fn append<'env>(&self, env: &'env Env, other: &FragmentHandle) -> Result<Unknown<'env>> {
        wrap(env, &self.fragment.append(&other.fragment))
    }

    #[napi]
    pub fn cut<'env>(&self, env: &'env Env, from: f64, to: f64) -> Result<Unknown<'env>> {
        let (from, to) = (js::pos(env, from)?, js::pos(env, to)?);
        wrap(env, &self.fragment.cut(from, to))
    }

    #[napi]
    pub fn cut_by_index<'env>(&self, env: &'env Env, from: u32, to: u32) -> Result<Unknown<'env>> {
        wrap(env, &self.fragment.cut_by_index(from as usize, to as usize))
    }

    #[napi]
    pub fn replace_child<'env>(
        &self,
        env: &'env Env,
        index: u32,
        node: &NodeHandle,
    ) -> Result<Unknown<'env>> {
        let replaced = self
            .fragment
            .replace_child(index as usize, node.node.clone());
        wrap(env, &replaced)
    }

    #[napi]
    pub fn add_to_start<'env>(&self, env: &'env Env, node: &NodeHandle) -> Result<Unknown<'env>> {
        wrap(env, &self.fragment.add_to_start(node.node.clone()))
    }

    #[napi]
    pub fn add_to_end<'env>(&self, env: &'env Env, node: &NodeHandle) -> Result<Unknown<'env>> {
        wrap(env, &self.fragment.add_to_end(node.node.clone()))
    }

    #[napi]
    pub fn eq(&self, other: &FragmentHandle) -> bool {
        self.fragment == other.fragment
    }

    #[napi]
    pub fn find_diff_start(
        &self,
        env: &Env,
        other: &FragmentHandle,
        pos: f64,
    ) -> Result<Option<u32>> {
        let start = self
            .fragment
            .find_diff_start(&other.fragment, js::pos(env, pos)?);
        Ok(start.map(|pos| pos as u32))
    }

    #[napi]
    pub fn find_diff_end(
        &self,
        env: &Env,
        other: &FragmentHandle,
        pos: f64,
        other_pos: f64,
    ) -> Result<Option<DiffEnd>> {
        let (pos, other_pos) = (js::pos(env, pos)?, js::pos(env, other_pos)?);
        let end = self.fragment.find_diff_end(&other.fragment, pos, other_pos);
        Ok(end.map(|(a, b)| DiffEnd {
            a: a as u32,
            b: b as u32,
        }))
    }

    #[napi]
    pub fn find_index(&self, env: &Env, pos: f64) -> Result<IndexInfo> {
        let (index, offset) = self.fragment.find_index(js::pos(env, pos)?).or_throw(env)?;
        Ok(IndexInfo {
            index: index as u32,
            offset: offset as u32,
        })
    }

    #[napi]
    pub fn to_debug_string(&self, env: &Env) -> Result<String> {
        self.fragment.to_debug_string().or_throw(env)
    }

    #[napi]
    pub fn to_string_inner(&self, env: &Env) -> Result<String> {
        self.fragment.to_string_inner().or_throw(env)
    }

    #[napi]
    pub fn to_json<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        js::value_to_js(env, &self.fragment.to_json())
    }
}

#[napi]
pub fn fragment_empty(env: &Env) -> Result<Unknown<'_>> {
    wrap(env, &Fragment::empty())
}

#[napi]
pub fn fragment_from_array<'env>(env: &'env Env, nodes: Vec<&NodeHandle>) -> Result<Unknown<'env>> {
    let nodes = nodes.into_iter().map(|node| node.node.clone()).collect();
    wrap(env, &Fragment::from_array(nodes))
}

#[napi]
pub fn fragment_from_json<'env>(
    env: &'env Env,
    schema: &SchemaHandle,
    json: Unknown,
) -> Result<Unknown<'env>> {
    let fragment = Fragment::from_json(&schema.schema, &js::json_from_js(json)?).or_throw(env)?;
    wrap(env, &fragment)
}
