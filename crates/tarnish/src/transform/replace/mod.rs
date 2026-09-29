//! Fitting a slice into a gap: `replaceStep`, and the looser `replaceRange` family.

mod fit;
mod range;

pub use fit::replace_step;

use crate::Result;
use crate::model::{ResolvedPos, Slice};

fn fits_trivially(from: &ResolvedPos, to: &ResolvedPos, slice: &Slice) -> Result<bool> {
    Ok(slice.open_start() == 0
        && slice.open_end() == 0
        && from.start(from.depth()) == to.start(to.depth())
        && from.parent().can_replace(
            from.index(from.depth()),
            to.index(to.depth()),
            slice.content(),
            0,
            slice.content().child_count(),
        )?)
}
