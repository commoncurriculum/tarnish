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

/// Runs `f` where at least `room` bytes of stack are left: for a recursion that can't go
/// through [`grow`] at each level, as another library's can't, given room for as deep as its
/// input makes it go.
#[inline]
pub fn with_room<R>(room: usize, f: impl FnOnce() -> R) -> R {
    stacker::maybe_grow(room, room.max(SEGMENT), f)
}

/// Runs `f` on a thread whose stack is smaller than a BEAM dirty CPU scheduler's 320 KiB, and
/// gives what it returns, or carries on its panic: a test runs a deep recursion there to show
/// that each of its levels goes through [`grow`].
pub fn on_dirty_scheduler_stack<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> R {
    std::thread::Builder::new()
        .stack_size(256 << 10)
        .spawn(f)
        .expect("a thread")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

/// Drops the items of a vector whose items nest, as deeply as they do.
pub fn drop_nested<T>(items: &mut Vec<T>) {
    if !items.is_empty() {
        let items = std::mem::take(items);
        grow(|| drop(items));
    }
}
