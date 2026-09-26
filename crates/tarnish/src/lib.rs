//! ProseMirror's document model and transforms, for servers: build and check documents against a
//! schema, and apply the steps an editor sends, without running JavaScript.
//!
//! Positions count UTF-16 units, as they do in the browser, so a step lands where it did there.

pub mod dom;
pub mod error;
pub mod json;
pub mod model;
pub mod text;
pub mod transform;
pub mod value;

pub use error::{Error, Result};
pub use model::*;
pub use text::Text;
pub use value::{Object, Value};
