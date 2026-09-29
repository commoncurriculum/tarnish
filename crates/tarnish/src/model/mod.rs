//! ProseMirror's document model: schemas, nodes, marks, fragments, slices and positions.

mod attrs;
mod check;
mod compact;
pub(crate) mod compare_deep;
mod content;
mod diff;
mod fields;
mod fragment;
mod mark;
mod node;
pub mod read;
mod replace;
mod resolved_pos;
mod schema;
mod schema_json;
mod view;

pub use attrs::{AttributeDefault, AttributeSpec, Attrs, Validate, ValidateHook};
pub use compare_deep::{compare_deep, objects_equal};
pub use content::{ContentExpr, ContentMatch};
pub use fields::{Field, Fields};
pub use fragment::{Fragment, LeafTextHook, NodeVisitor};
pub use mark::{Mark, Marks};
pub use node::{ChildAt, Node};
pub use replace::{REPLACE_ERROR, Slice};
pub use resolved_pos::{NodePredicate, NodeRange, ResolvedPos};
pub use schema::{
    MarkSpec, MarkType, NodeHook, NodeSpec, NodeType, Schema, SchemaSpec, Whitespace,
};
pub use view::{MarkRef, NodeId, NodeRef, SetRef, TextRef};
