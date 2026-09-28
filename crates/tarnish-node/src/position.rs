//! Resolved positions and node ranges.

use napi::bindgen_prelude::{FnArgs, Function, Null, ToNapiValue, Unknown};
use napi::{Env, JsValue, Result};
use napi_derive::napi;
use tarnish::{NodeRange, ResolvedPos};

use crate::js::{self, OrThrow};
use crate::mark;
use crate::node;

pub fn wrap<'env>(env: &'env Env, pos: ResolvedPos<'static>) -> Result<Unknown<'env>> {
    js::call_registered(env, "wrapResolvedPos", ResolvedPosHandle { pos })
}

/// A node range as JavaScript's `NodeRange`, of wrappers of its positions.
fn wrap_range<'env>(env: &'env Env, range: NodeRange<'static>) -> Result<Unknown<'env>> {
    let args = (
        wrap(env, range.resolved_from().clone())?,
        wrap(env, range.resolved_to().clone())?,
        range.depth() as f64,
    );
    js::call_registered(env, "wrapNodeRange", FnArgs::from(args))
}

#[napi]
pub struct ResolvedPosHandle {
    pub(crate) pos: ResolvedPos<'static>,
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
    pub fn node<'env>(&self, env: &'env Env, depth: u32) -> Result<Unknown<'env>> {
        node::wrap(env, self.pos.node(depth as usize))
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
    pub fn before(&self, env: &Env, depth: u32) -> Result<u32> {
        let before = self.pos.before(depth as usize).or_throw(env)?;
        Ok(before as u32)
    }

    #[napi]
    pub fn after(&self, env: &Env, depth: u32) -> Result<u32> {
        let after = self.pos.after(depth as usize).or_throw(env)?;
        Ok(after as u32)
    }

    #[napi(getter)]
    pub fn text_offset(&self) -> u32 {
        self.pos.text_offset() as u32
    }

    #[napi]
    pub fn node_after<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        node::wrap_option(env, self.pos.node_after().as_ref())
    }

    #[napi]
    pub fn node_before<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        node::wrap_option(env, self.pos.node_before().as_ref())
    }

    #[napi]
    pub fn pos_at_index(&self, index: u32, depth: u32) -> u32 {
        self.pos.pos_at_index(index as usize, depth as usize) as u32
    }

    #[napi]
    pub fn marks<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        mark::wrap_set(env, &self.pos.marks())
    }

    #[napi]
    pub fn marks_across<'env>(
        &self,
        env: &'env Env,
        end: &ResolvedPosHandle,
    ) -> Result<Unknown<'env>> {
        match self.pos.marks_across(&end.pos) {
            Some(marks) => mark::wrap_set(env, &marks),
            None => Null.into_unknown(env),
        }
    }

    #[napi]
    pub fn shared_depth(&self, env: &Env, pos: f64) -> Result<u32> {
        Ok(self.pos.shared_depth(js::pos(env, pos)?) as u32)
    }

    #[napi]
    pub fn block_range<'env>(
        &self,
        env: &'env Env,
        other: &ResolvedPosHandle,
        pred: Option<Function>,
    ) -> Result<Unknown<'env>> {
        let mut call = pred.map(|pred| {
            move |node: &tarnish::Node<'static>| {
                js::host(|env| {
                    js::call(pred.to_unknown(), node::wrap(env, node)?)?.coerce_to_bool()
                })
            }
        });
        let pred = call
            .as_mut()
            .map(|f| f as &mut tarnish::NodePredicate<'_, 'static>);
        match self.pos.block_range(&other.pos, pred).or_throw(env)? {
            Some(range) => wrap_range(env, range),
            None => Null.into_unknown(env),
        }
    }

    #[napi]
    pub fn same_parent(&self, other: &ResolvedPosHandle) -> bool {
        self.pos.same_parent(&other.pos)
    }

    #[napi]
    pub fn to_debug_string(&self) -> String {
        self.pos.to_string()
    }
}

/// A node range, which JavaScript makes into a handle the first time it passes one to the
/// bridge.
#[napi]
pub struct NodeRangeHandle {
    pub(crate) range: NodeRange<'static>,
}

#[napi]
impl NodeRangeHandle {
    #[napi(constructor)]
    pub fn new(from: &ResolvedPosHandle, to: &ResolvedPosHandle, depth: u32) -> Self {
        NodeRangeHandle {
            range: NodeRange::new(from.pos.clone(), to.pos.clone(), depth as usize),
        }
    }

    #[napi(getter)]
    pub fn start(&self) -> u32 {
        self.range.start() as u32
    }

    #[napi(getter)]
    pub fn end(&self) -> u32 {
        self.range.end() as u32
    }

    #[napi(getter)]
    pub fn parent<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        node::wrap(env, self.range.parent())
    }

    #[napi(getter)]
    pub fn start_index(&self) -> u32 {
        self.range.start_index() as u32
    }

    #[napi(getter)]
    pub fn end_index(&self) -> u32 {
        self.range.end_index() as u32
    }
}
