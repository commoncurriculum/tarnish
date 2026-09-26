//! Resolved positions and node ranges.

use napi::{Env, Result, sys};
use napi_derive::napi;
use tarnish::{NodeRange, ResolvedPos};

use crate::js::{self, Js, OrThrow};
use crate::mark;
use crate::node;

/// A position from JavaScript. One that is negative or not whole can't be in a document; it
/// becomes one past any document's end, so it is out of range.
pub fn pos_arg(pos: f64) -> usize {
    if pos >= 0.0 && pos.fract() == 0.0 {
        pos as usize
    } else {
        usize::MAX
    }
}

pub struct ResolvedPosArg(pub ResolvedPos);

crate::handle_arg!(ResolvedPosArg, ResolvedPosHandle, |handle| handle
    .pos
    .clone());

pub fn wrap(env: sys::napi_env, pos: ResolvedPos) -> Result<sys::napi_value> {
    let handle = unsafe {
        napi::bindgen_prelude::ToNapiValue::to_napi_value(env, ResolvedPosHandle { pos })
    }?;
    js::call_registered(env, "wrapResolvedPos", &[handle])
}

#[napi]
pub struct ResolvedPosHandle {
    pos: ResolvedPos,
}

#[napi]
impl ResolvedPosHandle {
    #[napi(getter)]
    pub fn pos(&self) -> u32 {
        self.pos.pos() as u32
    }

    #[napi(getter)]
    pub fn depth(&self) -> u32 {
        self.pos.depth() as u32
    }

    #[napi(getter)]
    pub fn parent_offset(&self) -> u32 {
        self.pos.parent_offset() as u32
    }

    #[napi]
    pub fn node(&self, env: Env, depth: u32) -> Result<Js> {
        node::wrap(env.raw(), self.pos.node(depth as usize)).map(Js)
    }

    #[napi]
    pub fn index(&self, depth: u32) -> u32 {
        self.pos.index(depth as usize) as u32
    }

    #[napi]
    pub fn index_after(&self, depth: u32) -> u32 {
        self.pos.index_after(depth as usize) as u32
    }

    #[napi]
    pub fn start(&self, depth: u32) -> u32 {
        self.pos.start(depth as usize) as u32
    }

    #[napi]
    pub fn end(&self, depth: u32) -> u32 {
        self.pos.end(depth as usize) as u32
    }

    #[napi]
    pub fn before(&self, env: Env, depth: u32) -> Result<u32> {
        self.pos
            .before(depth as usize)
            .map(|pos| pos as u32)
            .or_throw(&env)
    }

    #[napi]
    pub fn after(&self, env: Env, depth: u32) -> Result<u32> {
        self.pos
            .after(depth as usize)
            .map(|pos| pos as u32)
            .or_throw(&env)
    }

    #[napi(getter)]
    pub fn text_offset(&self) -> u32 {
        self.pos.text_offset() as u32
    }

    #[napi]
    pub fn node_after(&self, env: Env) -> Result<Js> {
        match self.pos.node_after() {
            Some(node) => node::wrap(env.raw(), &node).map(Js),
            None => js::null(env.raw()).map(Js),
        }
    }

    #[napi]
    pub fn node_before(&self, env: Env) -> Result<Js> {
        match self.pos.node_before() {
            Some(node) => node::wrap(env.raw(), &node).map(Js),
            None => js::null(env.raw()).map(Js),
        }
    }

    #[napi]
    pub fn pos_at_index(&self, index: u32, depth: u32) -> u32 {
        self.pos.pos_at_index(index as usize, depth as usize) as u32
    }

    #[napi]
    pub fn marks(&self, env: Env) -> Result<Js> {
        mark::wrap_set(env.raw(), &self.pos.marks()).map(Js)
    }

    #[napi]
    pub fn marks_across(&self, env: Env, end: ResolvedPosArg) -> Result<Js> {
        match self.pos.marks_across(&end.0) {
            Some(marks) => mark::wrap_set(env.raw(), &marks).map(Js),
            None => js::null(env.raw()).map(Js),
        }
    }

    #[napi]
    pub fn shared_depth(&self, pos: u32) -> u32 {
        self.pos.shared_depth(pos as usize) as u32
    }

    #[napi]
    pub fn block_range(&self, env: Env, other: ResolvedPosArg, pred: Option<Js>) -> Result<Js> {
        let raw = env.raw();
        let mut call = pred.map(|Js(pred)| {
            move |node: &tarnish::Node| {
                let result = (|| {
                    let result =
                        js::call(raw, js::undefined(raw)?, pred, &[node::wrap(raw, node)?])?;
                    js::truthy(raw, result)
                })();
                result.map_err(|error| js::host_error(raw, error))
            }
        });
        let pred = call.as_mut().map(|f| f as &mut tarnish::NodePredicate);
        match self.pos.block_range(&other.0, pred).or_throw(&env)? {
            Some(range) => wrap_range(raw, range).map(Js),
            None => js::null(raw).map(Js),
        }
    }

    #[napi]
    pub fn same_parent(&self, other: ResolvedPosArg) -> bool {
        self.pos.same_parent(&other.0)
    }

    #[napi]
    pub fn to_debug_string(&self) -> String {
        self.pos.to_string()
    }
}

/// A node range as JavaScript's `NodeRange`, of wrappers of its positions.
pub fn wrap_range(env: sys::napi_env, range: NodeRange) -> Result<sys::napi_value> {
    let args = [
        wrap(env, range.resolved_from().clone())?,
        wrap(env, range.resolved_to().clone())?,
        js::number(env, range.depth() as f64)?,
    ];
    js::call_registered(env, "wrapNodeRange", &args)
}
