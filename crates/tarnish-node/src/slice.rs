//! Slices, which JavaScript holds as plain objects of a fragment and two open depths.

use napi::bindgen_prelude::{FromNapiValue, TypeName, ValidateNapiValue};
use napi::{Env, Result, ValueType, sys};
use napi_derive::napi;
use tarnish::Slice;

use crate::fragment::{self, FragmentArg};
use crate::js::{self, Data, Js, OrThrow};
use crate::schema::SchemaHandle;

pub struct SliceArg(pub Slice);

impl FromNapiValue for SliceArg {
    unsafe fn from_napi_value(env: sys::napi_env, value: sys::napi_value) -> Result<Self> {
        let content =
            unsafe { FragmentArg::from_napi_value(env, js::get(env, value, "content")?) }?;
        let open_start = unsafe { u32::from_napi_value(env, js::get(env, value, "openStart")?) }?;
        let open_end = unsafe { u32::from_napi_value(env, js::get(env, value, "openEnd")?) }?;
        Ok(SliceArg(Slice::new(
            content.0,
            open_start as usize,
            open_end as usize,
        )))
    }
}

impl TypeName for SliceArg {
    fn type_name() -> &'static str {
        "Slice"
    }

    fn value_type() -> ValueType {
        ValueType::Object
    }
}

impl ValidateNapiValue for SliceArg {}

pub fn wrap(env: sys::napi_env, slice: &Slice) -> Result<sys::napi_value> {
    let args = [
        fragment::wrap(env, slice.content())?,
        js::number(env, slice.open_start() as f64)?,
        js::number(env, slice.open_end() as f64)?,
    ];
    js::call_registered(env, "wrapSlice", &args)
}

#[napi]
pub fn slice_size(slice: SliceArg) -> u32 {
    slice.0.size() as u32
}

#[napi]
pub fn slice_insert_at(env: Env, slice: SliceArg, pos: u32, fragment: FragmentArg) -> Result<Js> {
    match slice
        .0
        .insert_at(pos as usize, &fragment.0)
        .or_throw(&env)?
    {
        Some(slice) => wrap(env.raw(), &slice).map(Js),
        None => js::null(env.raw()).map(Js),
    }
}

#[napi]
pub fn slice_remove_between(env: Env, slice: SliceArg, from: u32, to: u32) -> Result<Js> {
    let removed = slice
        .0
        .remove_between(from as usize, to as usize)
        .or_throw(&env)?;
    wrap(env.raw(), &removed).map(Js)
}

#[napi]
pub fn slice_eq(slice: SliceArg, other: SliceArg) -> bool {
    slice.0 == other.0
}

#[napi]
pub fn slice_to_debug_string(env: Env, slice: SliceArg) -> Result<String> {
    slice.0.to_debug_string().or_throw(&env)
}

#[napi]
pub fn slice_to_json(slice: SliceArg) -> Data {
    Data(slice.0.to_json())
}

#[napi]
pub fn slice_from_json(env: Env, schema: &SchemaHandle, json: Data) -> Result<Js> {
    let slice = Slice::from_json(&schema.schema, &json.0).or_throw(&env)?;
    wrap(env.raw(), &slice).map(Js)
}

#[napi]
pub fn slice_max_open(env: Env, fragment: FragmentArg, open_isolating: bool) -> Result<Js> {
    wrap(env.raw(), &Slice::max_open(fragment.0, open_isolating)).map(Js)
}
