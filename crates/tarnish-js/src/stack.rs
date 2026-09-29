//! Room on the stack for recursions as deep as documents and values nest. Each level of such a
//! recursion runs through [`grow`], which carries on in a new stack segment when the thread's
//! stack runs low: a caller's stack may be small, as a BEAM dirty scheduler's is, and running
//! out of it would take the whole process down.

/// The most a level of recursion may take between calls to [`grow`].
const RED_ZONE: usize = 128 << 10;
/// The size of each new segment, whose pages the system commits only as they are used.
const SEGMENT: usize = 8 << 20;

#[inline]
pub fn grow<R>(f: impl FnOnce() -> R) -> R {
    stacker::maybe_grow(RED_ZONE, SEGMENT, f)
}

/// Drops the items of a vector whose items nest, as deeply as they do.
pub fn drop_nested<T>(items: &mut Vec<T>) {
    if !items.is_empty() {
        let items = std::mem::take(items);
        grow(|| drop(items));
    }
}
