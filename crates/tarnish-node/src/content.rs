//! Content matches.

use std::ptr;

use napi::{Env, Result, sys};
use napi_derive::napi;
use tarnish::ContentMatch;

use crate::fragment::{self, FragmentArg};
use crate::js::{self, Js, OrThrow};
use crate::schema::{self, NodeTypeArg, SchemaHandle};

pub struct ContentMatchArg(pub ContentMatch);

crate::handle_arg!(ContentMatchArg, ContentMatchHandle, |handle| handle
    .content_match
    .clone());

/// The wrapper of the match: one per state of an expression's automaton.
pub fn wrap(env: sys::napi_env, content_match: &ContentMatch) -> Result<sys::napi_value> {
    let (automaton, state) = content_match.id();
    let key = js::string(env, &format!("{automaton}:{state}"))?;
    let handle = ContentMatchHandle {
        content_match: content_match.clone(),
    };
    let handle = unsafe { napi::bindgen_prelude::ToNapiValue::to_napi_value(env, handle) }?;
    js::call_registered(env, "wrapContentMatch", &[handle, key])
}

fn wrap_option(env: sys::napi_env, content_match: Option<ContentMatch>) -> Result<Js> {
    match content_match {
        Some(content_match) => wrap(env, &content_match).map(Js),
        None => js::null(env).map(Js),
    }
}

#[napi(object)]
pub struct Edge {
    #[napi(js_name = "type")]
    pub node_type: Js,
    pub next: Js,
}

#[napi]
pub struct ContentMatchHandle {
    content_match: ContentMatch,
}

#[napi]
impl ContentMatchHandle {
    #[napi(getter)]
    pub fn valid_end(&self) -> bool {
        self.content_match.valid_end()
    }

    #[napi(getter)]
    pub fn inline_content(&self) -> bool {
        self.content_match.inline_content()
    }

    #[napi(getter)]
    pub fn edge_count(&self) -> u32 {
        self.content_match.edge_count() as u32
    }

    #[napi]
    pub fn match_type(&self, env: Env, node_type: NodeTypeArg) -> Result<Js> {
        wrap_option(env.raw(), self.content_match.match_type(&node_type.0))
    }

    #[napi]
    pub fn match_fragment(
        &self,
        env: Env,
        fragment: FragmentArg,
        start: u32,
        end: u32,
    ) -> Result<Js> {
        let matched = self
            .content_match
            .match_fragment(&fragment.0, start as usize, end as usize);
        wrap_option(env.raw(), matched)
    }

    #[napi]
    pub fn default_type(&self, env: Env) -> Result<Js> {
        match self.content_match.default_type() {
            Some(node_type) => schema::wrap_node_type(env.raw(), &node_type).map(Js),
            None => js::null(env.raw()).map(Js),
        }
    }

    #[napi]
    pub fn compatible(&self, other: ContentMatchArg) -> bool {
        self.content_match.compatible(&other.0)
    }

    #[napi]
    pub fn fill_before(
        &self,
        env: Env,
        after: FragmentArg,
        to_end: bool,
        start_index: u32,
    ) -> Result<Js> {
        let filled = self
            .content_match
            .fill_before(&after.0, to_end, start_index as usize)
            .or_throw(&env)?;
        match filled {
            Some(filled) => fragment::wrap(env.raw(), &filled).map(Js),
            None => js::null(env.raw()).map(Js),
        }
    }

    #[napi]
    pub fn find_wrapping(&self, env: Env, target: NodeTypeArg) -> Result<Js> {
        let env = env.raw();
        let Some(wrapping) = self.content_match.find_wrapping(&target.0) else {
            return js::null(env).map(Js);
        };
        let mut array = ptr::null_mut();
        js::check(unsafe { sys::napi_create_array_with_length(env, wrapping.len(), &mut array) })?;
        for (index, node_type) in wrapping.iter().enumerate() {
            let node_type = schema::wrap_node_type(env, node_type)?;
            js::check(unsafe { sys::napi_set_element(env, array, index as u32, node_type) })?;
        }
        Ok(Js(array))
    }

    #[napi]
    pub fn edge(&self, env: Env, n: u32) -> Result<Edge> {
        let (node_type, next) = self.content_match.edge(n as usize).or_throw(&env)?;
        Ok(Edge {
            node_type: Js(schema::wrap_node_type(env.raw(), &node_type)?),
            next: Js(wrap(env.raw(), &next)?),
        })
    }

    #[napi]
    pub fn to_debug_string(&self) -> String {
        self.content_match.to_string()
    }
}

#[napi]
pub fn content_match_parse(env: Env, schema: &SchemaHandle, expr: String) -> Result<Js> {
    let parsed = ContentMatch::parse(&schema.schema, &expr).or_throw(&env)?;
    wrap(env.raw(), &parsed).map(Js)
}
