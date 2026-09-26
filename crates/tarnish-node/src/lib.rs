//! The Node.js bridge that runs ProseMirror's own test suites against tarnish.
//!
//! JavaScript code never sees this crate's handles: `js/` wraps each in the class ProseMirror
//! exports and passes the bridge the handle a wrapper keeps in `h`. Every call returns the
//! wrapper, which the bridge asks the JavaScript side for so that one node is always one
//! object. All of ProseMirror's logic runs in tarnish; the bridge and `js/` only convert
//! arguments and results.

// Not `forbid`: every `#[napi]` item registers itself through the ctor crate, whose expansion
// has an `allow` for this lint that `forbid` rejects.
#![deny(unsafe_code)]
// napi registers exported functions only outside test builds, which leaves them unused there.
#![cfg_attr(test, allow(dead_code))]

mod content;
mod dom;
mod fragment;
mod js;
mod mark;
mod node;
mod position;
mod schema;
mod slice;
mod transform;

use napi::bindgen_prelude::Object;
use napi::{Env, Result};
use napi_derive::napi;

/// Give the bridge the JavaScript side's wrapping functions and error classes, by name.
#[napi]
pub fn register(env: Env, functions: Object) -> Result<()> {
    js::register(env, functions)
}
