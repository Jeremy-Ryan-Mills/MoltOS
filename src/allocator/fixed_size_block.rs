/***
 * src/allocator/fixed_size_block.rs
 *
 * Fixed-size block allocator with a linked-list fallback.
 * Maintains one free-list per block size class (BLOCK_SIZES). Allocations from
 * a matching size class are O(1). Larger allocations fall through to a
 * general-purpose heap (linked_list_allocator::Heap).
 */

use alloc::alloc::{GlobalAlloc, Layout};
use core::{mem, ptr};
use core::ptr::NonNull;
use super::Locked;

// Block sizes must be powers of two (used as alignment too).
const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048];

// Written into freed blocks to link them into the free-list.
struct ListNode {
    next: Option<&'static mut ListNode>,
}

pub struct FixedSizeBlockAllocator {
    list_heads: [Option<&'static mut ListNode>; BLOCK_SIZES.len()],
    fallback_allocator: linked_list_allocator::Heap,
}

impl FixedSizeBlockAllocator {
    pub const fn new() -> Self {
        const EMPTY: Option<&'static mut ListNode> = None;
        Self {
            list_heads: [EMPTY; BLOCK_SIZES.len()],
            fallback_allocator: linked_list_allocator::Heap::empty(),
        }
    }

    // Safety: heap_start..heap_start+heap_size must be valid unused memory.
    // Call exactly once.
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        unsafe { self.fallback_allocator.init(heap_start, heap_size); }
    }

    fn fallback_alloc(&mut self, layout: Layout) -> *mut u8 {
        match self.fallback_allocator.allocate_first_fit(layout) {
            Ok(ptr) => ptr.as_ptr(),
            Err(_) => ptr::null_mut(),
        }
    }
}

fn list_index(layout: &Layout) -> Option<usize> {
    let required = layout.size().max(layout.align());
    BLOCK_SIZES.iter().position(|&s| s >= required)
}

unsafe impl GlobalAlloc for Locked<FixedSizeBlockAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut allocator = self.lock();
        match list_index(&layout) {
            Some(index) => match allocator.list_heads[index].take() {
                Some(node) => {
                    allocator.list_heads[index] = node.next.take();
                    node as *mut ListNode as *mut u8
                }
                None => {
                    // Free list empty — get a new block from the fallback heap.
                    let block_size = BLOCK_SIZES[index];
                    let layout = Layout::from_size_align(block_size, block_size).unwrap();
                    allocator.fallback_alloc(layout)
                }
            },
            None => allocator.fallback_alloc(layout),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut allocator = self.lock();
        match list_index(&layout) {
            Some(index) => {
                assert!(mem::size_of::<ListNode>() <= BLOCK_SIZES[index]);
                assert!(mem::align_of::<ListNode>() <= BLOCK_SIZES[index]);
                let new_node = ListNode { next: allocator.list_heads[index].take() };
                let new_node_ptr = ptr as *mut ListNode;
                unsafe {
                    new_node_ptr.write(new_node);
                    allocator.list_heads[index] = Some(&mut *new_node_ptr);
                }
            }
            None => {
                let ptr = NonNull::new(ptr).expect("dealloc called with null pointer");
                unsafe { allocator.fallback_allocator.deallocate(ptr, layout); }
            }
        }
    }
}
