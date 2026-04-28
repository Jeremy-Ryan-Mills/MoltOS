// Bump (linear) allocator. Hands out memory by advancing a pointer.
// Dealloc is a no-op; memory is only reclaimed when all allocations are freed.
// Fast but fragments badly under mixed alloc/dealloc patterns.

use alloc::alloc::{GlobalAlloc, Layout};
use core::ptr;
use super::{align_up, Locked};

pub struct BumpAllocator {
    heap_start: usize,
    heap_end: usize,
    next: usize,
    allocations: usize,
}

impl BumpAllocator {
    pub const fn new() -> Self {
        Self { heap_start: 0, heap_end: 0, next: 0, allocations: 0 }
    }

    // Safety: heap_start..heap_start+heap_size must be valid unused memory.
    // Call exactly once.
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.heap_start = heap_start;
        self.heap_end = heap_start + heap_size;
        self.next = heap_start;
        self.allocations = 0;
    }
}

unsafe impl GlobalAlloc for Locked<BumpAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut bump = self.lock();
        let alloc_start = align_up(bump.next, layout.align());
        let alloc_end = match alloc_start.checked_add(layout.size()) {
            Some(end) => end,
            None => return ptr::null_mut(),
        };
        if alloc_end > bump.heap_end {
            return ptr::null_mut();
        }
        bump.next = alloc_end;
        bump.allocations += 1;
        alloc_start as *mut u8
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        let mut bump = self.lock();
        bump.allocations -= 1;
        // Reset bump pointer when all allocations have been freed.
        if bump.allocations == 0 {
            bump.next = bump.heap_start;
        }
    }
}
