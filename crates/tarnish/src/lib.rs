//! ProseMirror's document model and transforms, for servers: build and check documents against a
//! schema, and apply the steps an editor sends, without running JavaScript.
//!
//! Positions count UTF-16 units, as they do in the browser, so a step lands where it did there.
//! Documents may nest as deeply as memory allows.

#![forbid(unsafe_code)]

pub mod api;
pub mod chunk;
pub mod dom;
pub mod error;
pub mod js;
pub mod json;
pub mod model;
#[cfg(test)]
mod random;
pub mod stack;
pub mod text;
pub mod transform;

pub use error::{Error, Result};
pub use json::{Key, Map, Value};
pub use model::*;
pub use text::Text;
