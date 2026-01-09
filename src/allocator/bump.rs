//! A simple bump (linear) allocator.
//!
//! This allocator hands out memory by monotonically increasing a pointer
//! within a fixed heap region. Individual deallocations do nothing; memory
//! is only reclaimed when *all* allocations have been freed.
//!
//! This allocator is extremely fast and simple, but can suffer from
//! fragmentation if allocations and deallocations are interleaved.

use alloc::alloc::{GlobalAlloc, Layout};
use core::ptr;

use super::{align_up, Locked};

/// A bump (a.k.a. linear) allocator.
///
/// Allocation works by returning the next aligned address in the heap and
/// advancing an internal pointer. Deallocation is a no-op except for
/// bookkeeping; the heap is reset only when the allocation count reaches zero.
pub struct BumpAllocator {
    /// Start address of the heap region.
    heap_start: usize,
    /// End address of the heap region (exclusive).
    heap_end: usize,
    /// Next free byte to allocate from.
    next: usize,
    /// Number of currently active allocations.
    allocations: usize,
}

impl BumpAllocator {
    /// Create a new, uninitialized bump allocator.
    ///
    /// The allocator does not manage any memory until [`init`] is called.
    pub const fn new() -> Self {
        Self {
            heap_start: 0,
            heap_end: 0,
            next: 0,
            allocations: 0,
        }
    }

    /// Initialize the allocator with a heap region.
    ///
    /// # Safety
    /// - `heap_start..heap_start + heap_size` must refer to valid, unused memory.
    /// - This function must be called exactly once.
    /// - No allocations may be active when this is called.
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.heap_start = heap_start;
        self.heap_end = heap_start + heap_size;
        self.next = heap_start;
        self.allocations = 0;
    }
}

unsafe impl GlobalAlloc for Locked<BumpAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // Acquire exclusive access to the allocator state.
        let mut bump = self.lock();

        // Align the allocation start to the required alignment.
        let alloc_start = align_up(bump.next, layout.align());

        // Compute the end of the allocation, checking for overflow.
        let alloc_end = match alloc_start.checked_add(layout.size()) {
            Some(end) => end,
            None => return ptr::null_mut(),
        };

        // Check for out-of-memory.
        if alloc_end > bump.heap_end {
            ptr::null_mut()
        } else {
            bump.next = alloc_end;
            bump.allocations += 1;
            alloc_start as *mut u8
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Acquire exclusive access to the allocator state.
        let mut bump = self.lock();

        // Decrement the allocation count. When it reaches zero,
        // reset the bump pointer to reclaim all memory at once.
        bump.allocations -= 1;
        if bump.allocations == 0 {
            bump.next = bump.heap_start;
        }
    }
}
