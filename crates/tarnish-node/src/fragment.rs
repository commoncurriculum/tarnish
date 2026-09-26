//! Fragments.

use std::ptr;

use napi::{Env, Result, sys};
use napi_derive::napi;
use tarnish::Fragment;

use crate::js::{self, Data, Js, JsText, OrThrow};
use crate::node::{self, NodeArg};
use crate::schema::SchemaHandle;

pub struct FragmentArg(pub Fragment);

crate::handle_arg!(FragmentArg, FragmentHandle, |handle| handle
    .fragment
    .clone());

pub fn wrap(env: sys::napi_env, fragment: &Fragment) -> Result<sys::napi_value> {
    let handle = FragmentHandle {
        fragment: fragment.clone(),
    };
    let handle = unsafe { napi::bindgen_prelude::ToNapiValue::to_napi_value(env, handle) }?;
    let id = js::number(env, fragment.id() as f64)?;
    js::call_registered(env, "wrapFragment", &[handle, id])
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

    /// The children, as an array of wrappers.
    #[napi]
    pub fn children(&self, env: Env) -> Result<Js> {
        let env = env.raw();
        let mut array = ptr::null_mut();
        js::check(unsafe {
            sys::napi_create_array_with_length(env, self.fragment.child_count(), &mut array)
        })?;
        for (index, child) in self.fragment.children().iter().enumerate() {
            let child = node::wrap(env, child)?;
            js::check(unsafe { sys::napi_set_element(env, array, index as u32, child) })?;
        }
        Ok(Js(array))
    }

    #[napi]
    pub fn child(&self, env: Env, index: u32) -> Result<Js> {
        let child = self.fragment.child(index as usize).or_throw(&env)?;
        node::wrap(env.raw(), child).map(Js)
    }

    #[napi]
    pub fn nodes_between(
        &self,
        env: Env,
        from: u32,
        to: u32,
        f: Js,
        node_start: u32,
        parent: Option<NodeArg>,
    ) -> Result<()> {
        let mut visit = node::visitor(env.raw(), f.0);
        let parent = parent.map(|parent| parent.0);
        self.fragment
            .nodes_between(
                from as usize,
                to as usize,
                &mut visit,
                node_start as usize,
                parent.as_ref(),
            )
            .or_throw(&env)
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
        let separator = node::separator(env.raw(), block_separator)?;
        let mut leaf_text = node::leaf_text_hook(env.raw(), leaf_text)?;
        let leaf_text = leaf_text.as_mut().map(|f| f as &mut tarnish::LeafTextHook);
        self.fragment
            .text_between(from as usize, to as usize, separator.as_ref(), leaf_text)
            .map(JsText)
            .or_throw(&env)
    }

    #[napi]
    pub fn for_each(&self, env: Env, f: Js) -> Result<()> {
        let env = env.raw();
        for (index, (offset, child)) in self.fragment.children_with_offsets().enumerate() {
            let args = [
                node::wrap(env, child)?,
                js::number(env, offset as f64)?,
                js::number(env, index as f64)?,
            ];
            js::call(env, js::undefined(env)?, f.0, &args)?;
        }
        Ok(())
    }

    #[napi]
    pub fn append(&self, env: Env, other: FragmentArg) -> Result<Js> {
        wrap(env.raw(), &self.fragment.append(&other.0)).map(Js)
    }

    #[napi]
    pub fn cut(&self, env: Env, from: u32, to: u32) -> Result<Js> {
        wrap(env.raw(), &self.fragment.cut(from as usize, to as usize)).map(Js)
    }

    #[napi]
    pub fn cut_by_index(&self, env: Env, from: u32, to: u32) -> Result<Js> {
        wrap(
            env.raw(),
            &self.fragment.cut_by_index(from as usize, to as usize),
        )
        .map(Js)
    }

    #[napi]
    pub fn replace_child(&self, env: Env, index: u32, node: NodeArg) -> Result<Js> {
        wrap(
            env.raw(),
            &self.fragment.replace_child(index as usize, node.0),
        )
        .map(Js)
    }

    #[napi]
    pub fn add_to_start(&self, env: Env, node: NodeArg) -> Result<Js> {
        wrap(env.raw(), &self.fragment.add_to_start(node.0)).map(Js)
    }

    #[napi]
    pub fn add_to_end(&self, env: Env, node: NodeArg) -> Result<Js> {
        wrap(env.raw(), &self.fragment.add_to_end(node.0)).map(Js)
    }

    #[napi]
    pub fn eq(&self, other: FragmentArg) -> bool {
        self.fragment == other.0
    }

    #[napi]
    pub fn find_diff_start(&self, other: FragmentArg, pos: u32) -> Option<u32> {
        self.fragment
            .find_diff_start(&other.0, pos as usize)
            .map(|pos| pos as u32)
    }

    #[napi]
    pub fn find_diff_end(&self, other: FragmentArg, pos: u32, other_pos: u32) -> Option<DiffEnd> {
        self.fragment
            .find_diff_end(&other.0, pos as usize, other_pos as usize)
            .map(|(a, b)| DiffEnd {
                a: a as u32,
                b: b as u32,
            })
    }

    #[napi]
    pub fn find_index(&self, env: Env, pos: u32) -> Result<IndexInfo> {
        let (index, offset) = self.fragment.find_index(pos as usize).or_throw(&env)?;
        Ok(IndexInfo {
            index: index as u32,
            offset: offset as u32,
        })
    }

    #[napi]
    pub fn to_debug_string(&self, env: Env) -> Result<String> {
        self.fragment.to_debug_string().or_throw(&env)
    }

    #[napi]
    pub fn to_string_inner(&self, env: Env) -> Result<String> {
        self.fragment.to_string_inner().or_throw(&env)
    }

    #[napi]
    pub fn to_json(&self) -> Data {
        Data(self.fragment.to_json())
    }
}

#[napi]
pub fn fragment_empty(env: Env) -> Result<Js> {
    wrap(env.raw(), &Fragment::empty()).map(Js)
}

#[napi]
pub fn fragment_from_array(env: Env, nodes: Vec<NodeArg>) -> Result<Js> {
    let nodes = nodes.into_iter().map(|node| node.0).collect();
    wrap(env.raw(), &Fragment::from_array(nodes)).map(Js)
}

#[napi]
pub fn fragment_from_json(env: Env, schema: &SchemaHandle, json: Data) -> Result<Js> {
    let fragment = Fragment::from_json(&schema.schema, &json.0).or_throw(&env)?;
    wrap(env.raw(), &fragment).map(Js)
}
