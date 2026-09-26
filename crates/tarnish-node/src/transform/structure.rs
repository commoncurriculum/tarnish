//! Replacing and restructuring: where steps fit, and the wrappers and marks transforms take.

use napi::bindgen_prelude::{ClassInstance, Either, Null, ToNapiValue, Unknown};
use napi::{Env, Result};
use napi_derive::napi;
use tarnish::transform::{MarkMatch, Wrapper};

use super::step;
use crate::js::{self, OrThrow};
use crate::mark::MarkHandle;
use crate::node::NodeHandle;
use crate::position::NodeRangeHandle;
use crate::schema::{self, MarkTypeHandle, NodeTypeHandle};
use crate::slice::SliceHandle;

/// A `{type, attrs}` wrapper, with the handle of its type.
#[napi(object)]
pub struct WrapperArg<'env> {
    #[napi(js_name = "type")]
    pub node_type: ClassInstance<'env, NodeTypeHandle>,
    pub attrs: Option<Unknown<'env>>,
}

/// Wrappers, `None` for a falsy entry.
pub fn wrappers(list: Vec<Option<WrapperArg>>) -> Result<Vec<Option<Wrapper>>> {
    list.into_iter()
        .map(|wrapper| {
            let Some(wrapper) = wrapper else {
                return Ok(None);
            };
            let attrs = wrapper.attrs.map(js::attrs_from_js).transpose()?;
            Ok(Some(Wrapper {
                node_type: wrapper.node_type.node_type.clone(),
                attrs: attrs.flatten(),
            }))
        })
        .collect()
}

pub fn mark_match<'a>(mark: &Either<&'a MarkHandle, &'a MarkTypeHandle>) -> MarkMatch<'a> {
    match mark {
        Either::A(mark) => MarkMatch::Mark(&mark.mark),
        Either::B(mark_type) => MarkMatch::Type(&mark_type.mark_type),
    }
}

#[napi(object)]
pub struct FoundWrapper<'env> {
    #[napi(js_name = "type")]
    pub node_type: Unknown<'env>,
    pub attrs: Unknown<'env>,
}

#[napi]
pub fn replace_step<'env>(
    env: &'env Env,
    doc: &NodeHandle,
    from: f64,
    to: f64,
    slice: &SliceHandle,
) -> Result<Unknown<'env>> {
    let (from, to) = (js::pos(env, from)?, js::pos(env, to)?);
    let replaced =
        tarnish::transform::replace_step(&doc.node, from, to, &slice.slice).or_throw(env)?;
    step::wrap_option(env, replaced)
}

#[napi]
pub fn can_join(env: &Env, doc: &NodeHandle, pos: f64) -> Result<bool> {
    tarnish::transform::can_join(&doc.node, js::pos(env, pos)?).or_throw(env)
}

#[napi]
pub fn can_split(
    env: &Env,
    doc: &NodeHandle,
    pos: f64,
    depth: u32,
    types_after: Option<Vec<Option<WrapperArg>>>,
) -> Result<bool> {
    let types = types_after.map(wrappers).transpose()?;
    let pos = js::pos(env, pos)?;
    tarnish::transform::can_split(&doc.node, pos, depth as usize, types.as_deref()).or_throw(env)
}

#[napi]
pub fn join_point(env: &Env, doc: &NodeHandle, pos: f64, dir: i32) -> Result<Option<u32>> {
    let point = tarnish::transform::join_point(&doc.node, js::pos(env, pos)?, dir).or_throw(env)?;
    Ok(point.map(|pos| pos as u32))
}

#[napi]
pub fn insert_point(
    env: &Env,
    doc: &NodeHandle,
    pos: f64,
    node_type: &NodeTypeHandle,
) -> Result<Option<u32>> {
    let pos = js::pos(env, pos)?;
    let point =
        tarnish::transform::insert_point(&doc.node, pos, &node_type.node_type).or_throw(env)?;
    Ok(point.map(|pos| pos as u32))
}

#[napi]
pub fn drop_point(
    env: &Env,
    doc: &NodeHandle,
    pos: f64,
    slice: &SliceHandle,
) -> Result<Option<u32>> {
    let pos = js::pos(env, pos)?;
    let point = tarnish::transform::drop_point(&doc.node, pos, &slice.slice).or_throw(env)?;
    Ok(point.map(|pos| pos as u32))
}

#[napi]
pub fn lift_target(env: &Env, range: &NodeRangeHandle) -> Result<Option<u32>> {
    let target = tarnish::transform::lift_target(&range.range).or_throw(env)?;
    Ok(target.map(|depth| depth as u32))
}

#[napi]
pub fn find_wrapping<'env>(
    env: &'env Env,
    range: &NodeRangeHandle,
    node_type: &NodeTypeHandle,
    attrs: Unknown,
    inner_range: &NodeRangeHandle,
) -> Result<Option<Vec<FoundWrapper<'env>>>> {
    let attrs = js::attrs_from_js(attrs)?;
    let found = tarnish::transform::find_wrapping(
        &range.range,
        &node_type.node_type,
        attrs,
        Some(&inner_range.range),
    )
    .or_throw(env)?;
    let Some(found) = found else {
        return Ok(None);
    };
    found
        .iter()
        .map(|wrapper| {
            Ok(FoundWrapper {
                node_type: schema::wrap_node_type(env, &wrapper.node_type)?,
                attrs: match &wrapper.attrs {
                    Some(attrs) => js::attrs_to_js(env, attrs)?,
                    None => Null.into_unknown(env)?,
                },
            })
        })
        .collect::<Result<_>>()
        .map(Some)
}
