//! ProseMirror's transforms: steps, the maps of the positions they change, and the operations
//! built from them.

mod map;
mod mark;
mod replace;
mod step;
mod structure;
#[allow(clippy::module_inception)]
mod transform;

pub use map::{MapResult, Mappable, Mapping, MappingSlice, Recover, StepMap};
pub use mark::MarkMatch;
pub use replace::replace_step;
pub use step::{MarkOp, Step, StepResult};
pub use structure::{
    BlockAttrs, Wrapper, can_join, can_split, drop_point, find_wrapping, insert_point, join_point,
    lift_target,
};
pub use transform::Transform;
