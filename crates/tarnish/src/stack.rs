//! A stack of its own for each call from another language, deep enough for any document
//! JavaScript could hold, with checks that throw `RangeError` before a recursion overflows it.
//! A caller's own stack may be small, as a BEAM scheduler's is, and overflowing it would take
//! the whole process down.

use std::cell::{Cell, RefCell};
use std::panic::{self, AssertUnwindSafe};

use crate::error::{Error, Result};

const STACK_SIZE: usize = 256 << 20;
/// What the stack keeps past the last check, for walks that recurse without checking.
const RED_ZONE: usize = 32 << 20;
/// How much of the stack a deep call leaves in memory; past it, pages are released.
const RESIDENT: usize = 1 << 20;

/// A thread's stack: a mapping whose lowest page is a guard.
struct Stack {
    mapping: *mut u8,
    page: usize,
}

impl Stack {
    fn new() -> Stack {
        // SAFETY: an anonymous private mapping, checked below; the guard page is inside it.
        unsafe {
            let page = libc::sysconf(libc::_SC_PAGESIZE) as usize;
            let mapping = libc::mmap(
                std::ptr::null_mut(),
                STACK_SIZE,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
                -1,
                0,
            );
            assert!(mapping != libc::MAP_FAILED, "mapping a stack");
            assert_eq!(libc::mprotect(mapping, page, libc::PROT_NONE), 0);
            Stack {
                mapping: mapping.cast(),
                page,
            }
        }
    }

    /// The lowest address checks let the stack reach.
    fn limit(&self) -> usize {
        self.mapping as usize + self.page + RED_ZONE
    }

    /// Hands back the pages below the resident part, if the stack reached them.
    fn release_below(&self, lowest: usize) {
        let top = self.mapping as usize + STACK_SIZE;
        let resident = top - RESIDENT;
        if lowest >= resident {
            return;
        }
        let start = self.mapping as usize + self.page;
        // SAFETY: the range lies inside the mapping, below anything the stack still uses.
        unsafe {
            libc::madvise(start as *mut _, resident - start, libc::MADV_DONTNEED);
        }
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        // SAFETY: the mapping is this stack's, and nothing runs on it once its thread exits.
        unsafe {
            libc::munmap(self.mapping.cast(), STACK_SIZE);
        }
    }
}

thread_local! {
    static STACK: RefCell<Option<Stack>> = const { RefCell::new(None) };
    /// While a call runs on the stack, the lowest address a check lets it reach.
    static LIMIT: Cell<usize> = const { Cell::new(0) };
    /// The lowest address a check saw the stack at.
    static LOWEST: Cell<usize> = const { Cell::new(usize::MAX) };
}

/// Runs `f` on the thread's own stack, or directly if it's on it already.
pub fn run<R>(f: impl FnOnce() -> R) -> R {
    if LIMIT.get() != 0 {
        return f();
    }
    STACK.with_borrow_mut(|stack| {
        let stack = stack.get_or_insert_with(Stack::new);
        LIMIT.set(stack.limit());
        LOWEST.set(usize::MAX);
        let base = stack.mapping.wrapping_add(stack.page);
        // SAFETY: the stack is aligned to a page and lies inside the mapping, and the callback
        // doesn't unwind: a panic is caught on the stack and resumed off it.
        let result = unsafe {
            psm::on_stack(base, STACK_SIZE - stack.page, || {
                panic::catch_unwind(AssertUnwindSafe(f))
            })
        };
        LIMIT.set(0);
        stack.release_below(LOWEST.get());
        result.unwrap_or_else(|payload| panic::resume_unwind(payload))
    })
}

/// Throws, as JavaScript does when its stack runs out, if the stack is nearly used up. Off the
/// stack [`run`] gives, as in Rust tests, it checks nothing.
#[inline]
pub fn check() -> Result<()> {
    let limit = LIMIT.get();
    if limit == 0 {
        return Ok(());
    }
    let pointer = psm::stack_pointer() as usize;
    if pointer < LOWEST.get() {
        LOWEST.set(pointer);
    }
    if pointer < limit {
        return Err(Error::Range("Maximum call stack size exceeded".into()));
    }
    Ok(())
}
