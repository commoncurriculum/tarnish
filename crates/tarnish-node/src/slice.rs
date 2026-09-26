//! Slices, which JavaScript holds as plain objects of a fragment and two open depths, and makes
//! into a handle the first time it passes one to the bridge.

use napi::bindgen_prelude::{FnArgs, Null, ToNapiValue, Unknown};
use napi::{Env, Result};
use napi_derive::napi;
use tarnish::Slice;

use crate::fragment::{self, FragmentHandle};
use crate::js::{self, OrThrow};
use crate::schema::SchemaHandle;

pub fn wrap<'env>(env: &'env Env, slice: &Slice) -> Result<Unknown<'env>> {
    let args = (
        fragment::wrap(env, slice.content())?,
        slice.open_start() as f64,
        slice.open_end() as f64,
    );
    js::call_registered(env, "wrapSlice", FnArgs::from(args))
}

#[napi]
pub struct SliceHandle {
    pub(crate) slice: Slice,
}

#[napi]
impl SliceHandle {
    #[napi(constructor)]
    pub fn new(content: &FragmentHandle, open_start: u32, open_end: u32) -> Self {
        SliceHandle {
            slice: Slice::new(
                content.fragment.clone(),
                open_start as usize,
                open_end as usize,
            ),
        }
    }

    #[napi(getter)]
    pub fn size(&self) -> u32 {
        self.slice.size() as u32
    }

    #[napi]
    pub fn insert_at<'env>(
        &self,
        env: &'env Env,
        pos: f64,
        fragment: &FragmentHandle,
    ) -> Result<Unknown<'env>> {
        let inserted = self
            .slice
            .insert_at(js::pos(env, pos)?, &fragment.fragment)
            .or_throw(env)?;
        match inserted {
            Some(slice) => wrap(env, &slice),
            None => Null.into_unknown(env),
        }
    }

    #[napi]
    pub fn remove_between<'env>(
        &self,
        env: &'env Env,
        from: f64,
        to: f64,
    ) -> Result<Unknown<'env>> {
        let removed = self
            .slice
            .remove_between(js::pos(env, from)?, js::pos(env, to)?)
            .or_throw(env)?;
        wrap(env, &removed)
    }

    #[napi]
    pub fn eq(&self, other: &SliceHandle) -> bool {
        self.slice == other.slice
    }

    #[napi]
    pub fn to_debug_string(&self, env: &Env) -> Result<String> {
        self.slice.to_debug_string().or_throw(env)
    }

    #[napi]
    pub fn to_json<'env>(&self, env: &'env Env) -> Result<Unknown<'env>> {
        js::value_to_js(env, &self.slice.to_json())
    }
}

#[napi]
pub fn slice_from_json<'env>(
    env: &'env Env,
    schema: &SchemaHandle,
    json: Unknown,
) -> Result<Unknown<'env>> {
    let slice = Slice::from_json(&schema.schema, &js::json_from_js(json)?).or_throw(env)?;
    wrap(env, &slice)
}

#[napi]
pub fn slice_max_open<'env>(
    env: &'env Env,
    fragment: &FragmentHandle,
    open_isolating: bool,
) -> Result<Unknown<'env>> {
    wrap(
        env,
        &Slice::max_open(fragment.fragment.clone(), open_isolating),
    )
}
