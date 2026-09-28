//! ProseMirror's document model and transforms, for servers: build and check documents against a
//! schema, and apply the steps an editor sends, without running JavaScript.
//!
//! Positions count UTF-16 units, as they do in the browser, so a step lands where it did there.
//! Documents may nest as deeply as memory allows.
//!
//! ```
//! use tarnish::transform::Transform;
//! use tarnish::{Node, api, json};
//!
//! let spec = r#"{"nodes": {"doc": {"content": "paragraph+"}, "paragraph": {"content": "text*"},
//!                          "text": {}},
//!                "marks": {"em": {}}}"#;
//! let schema = api::schema(&json::from_str(spec).expect("JSON"))?;
//! let doc = r#"{"type": "doc", "content": [
//!     {"type": "paragraph", "content": [{"type": "text", "text": "Hello"}]}]}"#;
//! let doc = Node::from_json(&schema, &json::from_str(doc).expect("JSON"))?;
//!
//! // Apply the steps an editor sent.
//! let steps = r#"[{"stepType": "replace", "from": 6, "to": 6,
//!                  "slice": {"content": [{"type": "text", "text": ", world"}]}}]"#;
//! let doc = api::apply_steps(&doc, &json::from_str(steps).expect("JSON"))?;
//! assert_eq!(doc.text_content()?.as_str(), Some("Hello, world"));
//!
//! // Or change it on the server, as a transform.
//! let em = schema.mark(&schema.mark_type("em").expect("em"), None)?;
//! let mut tr = Transform::new(doc);
//! tr.add_mark(1, 6, &em)?;
//! assert_eq!(
//!     tr.doc().to_json_string(),
//!     r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","marks":[{"type":"em"}],"text":"Hello"},{"type":"text","text":", world"}]}]}"#
//! );
//! assert_eq!(tr.steps()[0].to_json()["stepType"], "addMark");
//! # Ok::<(), tarnish::Error>(())
//! ```

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
