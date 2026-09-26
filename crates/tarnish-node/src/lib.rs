//! The Node.js bridge that runs ProseMirror's own test suites against tarnish.
//!
//! JavaScript code never sees this crate's handles: `js/` wraps each in the class ProseMirror
//! exports, and every call returns the wrapper, which the bridge asks the JavaScript side for so
//! that one node is always one object. All of ProseMirror's logic runs in tarnish; the bridge
//! and `js/` only convert arguments and results.

// napi registers exported functions only outside test builds, which leaves them unused there.
#![cfg_attr(test, allow(dead_code))]

mod content;
mod fragment;
mod js;
mod mark;
mod node;
mod position;
mod schema;
mod slice;

use napi::{Env, Result};
use napi_derive::napi;

/// Give the bridge the JavaScript side's wrapping functions and error classes, by name.
#[napi]
pub fn register(env: Env, functions: js::Js) -> Result<()> {
    js::register(env.raw(), functions.0)
}

/// `FromNapiValue` for an argument that is one of `js/`'s wrappers, read through the handle
/// in its `h` property.
#[macro_export]
macro_rules! handle_arg {
    ($arg:ident, $handle:ty, |$h:ident| $extract:expr) => {
        impl napi::bindgen_prelude::FromNapiValue for $arg {
            unsafe fn from_napi_value(
                env: napi::sys::napi_env,
                value: napi::sys::napi_value,
            ) -> napi::Result<Self> {
                let handle = $crate::js::get(env, value, "h")?;
                let $h = unsafe {
                    <&$handle as napi::bindgen_prelude::FromNapiValue>::from_napi_value(env, handle)
                }?;
                Ok($arg($extract))
            }
        }

        impl napi::bindgen_prelude::TypeName for $arg {
            fn type_name() -> &'static str {
                stringify!($arg)
            }

            fn value_type() -> napi::ValueType {
                napi::ValueType::Object
            }
        }

        impl napi::bindgen_prelude::ValidateNapiValue for $arg {}
    };
}
