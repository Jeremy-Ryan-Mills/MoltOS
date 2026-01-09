//! Fixed-size block allocator with a linked-list fallback.
//!
//! This allocator maintains multiple free-lists (one per block size) for fast
//! allocation/deallocation of common small sizes. If a request does not fit
//! into any supported block size, it falls back to a general-purpose heap
//! allocator (`linked_list_allocator::Heap`).
//!
//! Strategy:
//! - For each size class in [`BLOCK_SIZES`], keep a singly-linked free list.
//! - `alloc` pops from the free list if available; otherwise it requests a new
//!   block of that size from the fallback allocator.
//! - `dealloc` pushes the block back onto the appropriate free list; if the
//!   layout does not match any size class, the pointer is returned to the
//!   fallback allocator.

use alloc::alloc::{GlobalAlloc, Layout};
use core::{mem, ptr};
use core::ptr::NonNull;

use super::Locked;

/// A node stored inside freed blocks for a given size class.
///
/// When a block is freed, we reuse its memory to store a `ListNode` that links
/// it into the free list for its size class.
///
/// # Safety notes
/// - The block size and alignment must be sufficient to store a `ListNode`.
///   We enforce this in `dealloc` with assertions.
struct ListNode {
    /// Next free block in this size class.
    next: Option<&'static mut ListNode>,
}

/// The set of block sizes (size classes) supported by this allocator.
///
/// Each block size must be a power of two because it is also used as the block
/// alignment (and alignments must be powers of two).
///
/// If an allocation request needs more space or alignment than the largest size
/// here, it will use the fallback allocator instead.
const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048];

/// A fixed-size block allocator with multiple free-lists and a fallback heap.
///
/// `list_heads[i]` is the head of the free list for blocks of size
/// `BLOCK_SIZES[i]`.
pub struct FixedSizeBlockAllocator {
    /// One free-list head per size class.
    list_heads: [Option<&'static mut ListNode>; BLOCK_SIZES.len()],
    /// General-purpose allocator used when a size class is empty, or when a
    /// request doesn't fit any size class.
    fallback_allocator: linked_list_allocator::Heap,
}

impl FixedSizeBlockAllocator {
    /// Create a new, empty allocator.
    ///
    /// The allocator must be initialized with [`init`] before use.
    pub const fn new() -> Self {
        const EMPTY: Option<&'static mut ListNode> = None;
        Self {
            list_heads: [EMPTY; BLOCK_SIZES.len()],
            fallback_allocator: linked_list_allocator::Heap::empty(),
        }
    }

    /// Initialize the allocator with a heap region.
    ///
    /// This sets up the fallback allocator. The fixed-size lists begin empty
    /// and will be populated lazily as allocations/deallocations occur.
    ///
    /// # Safety
    /// - `heap_start..heap_start + heap_size` must be valid, unused memory.
    /// - Must be called exactly once.
    /// - No allocations may be active when this is called.
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.fallback_allocator.init(heap_start, heap_size);
    }

    /// Allocate using the fallback allocator.
    ///
    /// Returns a null pointer on allocation failure.
    fn fallback_alloc(&mut self, layout: Layout) -> *mut u8 {
        match self.fallback_allocator.allocate_first_fit(layout) {
            Ok(ptr) => ptr.as_ptr(),
            Err(_) => ptr::null_mut(),
        }
    }
}

/// Choose an appropriate size class for `layout`.
///
/// The required block size must satisfy both:
/// - `block_size >= layout.size()`
/// - `block_size >= layout.align()`
///
/// Returns `Some(index)` where `index` is into [`BLOCK_SIZES`], or `None` if no
/// size class can satisfy the request.
fn list_index(layout: &Layout) -> Option<usize> {
    let required_block_size = layout.size().max(layout.align());
    BLOCK_SIZES.iter().position(|&s| s >= required_block_size)
}

unsafe impl GlobalAlloc for Locked<FixedSizeBlockAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut allocator = self.lock();

        match list_index(&layout) {
            Some(index) => match allocator.list_heads[index].take() {
                // Reuse an existing block from the free list.
                Some(node) => {
                    allocator.list_heads[index] = node.next.take();
                    node as *mut ListNode as *mut u8
                }
                // Free list empty: request a new block from the fallback heap.
                None => {
                    let block_size = BLOCK_SIZES[index];
                    let block_align = block_size; // power-of-two invariant
                    let layout = Layout::from_size_align(block_size, block_align).unwrap();
                    allocator.fallback_alloc(layout)
                }
            },
            // Not a supported size class: use fallback heap directly.
            None => allocator.fallback_alloc(layout),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut allocator = self.lock();

        match list_index(&layout) {
            Some(index) => {
                // Ensure the freed block can store our ListNode.
                assert!(mem::size_of::<ListNode>() <= BLOCK_SIZES[index]);
                assert!(mem::align_of::<ListNode>() <= BLOCK_SIZES[index]);

                // Push the block onto the free list by writing a ListNode into it.
                let new_node = ListNode {
                    next: allocator.list_heads[index].take(),
                };

                let new_node_ptr = ptr as *mut ListNode;
                new_node_ptr.write(new_node);
                allocator.list_heads[index] = Some(&mut *new_node_ptr);
            }
            None => {
                // Not a supported size class: return it to the fallback allocator.
                let ptr = NonNull::new(ptr).expect("dealloc called with null pointer");
                allocator.fallback_allocator.deallocate(ptr, layout);
            }
        }
    }
}
