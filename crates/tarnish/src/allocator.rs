//! mimalloc, for the bindings to make their global allocator: the model allocates for every node,
//! and mimalloc makes that about twice as fast as the system's `malloc`. Its plain entry points
//! are used wherever they give the alignment asked for, since its aligned ones check the
//! alignment on every call.

use std::alloc::{GlobalAlloc, Layout};
use std::ffi::c_void;

use libmimalloc_sys as mi;

pub struct MiMalloc;

/// Whether `malloc` alone gives the layout's alignment, by the rule the standard library's
/// `System` allocator follows: it aligns every block for any type of its size, up to 16 bytes.
fn plain(layout: Layout) -> bool {
    layout.align() <= 16 && layout.align() <= layout.size()
}

// SAFETY: each method passes its caller's contract on to the mimalloc function for it, and
// memory from any of them may be freed or reallocated by any other.
unsafe impl GlobalAlloc for MiMalloc {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe {
            if plain(layout) {
                mi::mi_malloc(layout.size()).cast()
            } else {
                mi::mi_malloc_aligned(layout.size(), layout.align()).cast()
            }
        }
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe {
            if plain(layout) {
                mi::mi_zalloc(layout.size()).cast()
            } else {
                mi::mi_zalloc_aligned(layout.size(), layout.align()).cast()
            }
        }
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        unsafe { mi::mi_free(ptr.cast::<c_void>()) }
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe {
            let new = Layout::from_size_align_unchecked(new_size, layout.align());
            if plain(layout) && plain(new) {
                mi::mi_realloc(ptr.cast(), new_size).cast()
            } else {
                mi::mi_realloc_aligned(ptr.cast(), new_size, layout.align()).cast()
            }
        }
    }
}
