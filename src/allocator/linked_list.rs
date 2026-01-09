//! A simple free-list (linked list) allocator.
//!
//! This allocator tracks free regions in a singly-linked list, where each free
//! region begins with a [`ListNode`] stored *inside the free memory itself*.
//!
//! Allocation strategy (first-fit):
//! - Walk the free list and pick the first region that can satisfy the request
//!   (size + alignment).
//! - Split the region if there is leftover space; the remainder becomes a new
//!   free region.
//!
//! Deallocation:
//! - The freed block is simply inserted back into the free list as a new free
//!   region.
//!
//! Notes / tradeoffs:
//! - Simple and compact; good for early kernels.
//! - Can fragment over time (no coalescing/merging of adjacent free regions).

use alloc::alloc::{GlobalAlloc, Layout};
use core::{mem, ptr};

use super::{align_up, Locked};

/// Metadata stored at the start of each free region.
///
/// Because `ListNode` is written into freed memory, any free region must be:
/// - aligned to `align_of::<ListNode>()`
/// - large enough to hold at least one `ListNode`
struct ListNode {
    /// Size of the free region in bytes.
    size: usize,
    /// Next free region in the list.
    next: Option<&'static mut ListNode>,
}

impl ListNode {
    /// Create a new node with `size` bytes and no next pointer.
    const fn new(size: usize) -> Self {
        Self { size, next: None }
    }

    /// Start address of this region (the address where this node is stored).
    fn start_addr(&self) -> usize {
        self as *const Self as usize
    }

    /// End address of this region (exclusive).
    fn end_addr(&self) -> usize {
        self.start_addr() + self.size
    }
}

/// A linked-list based allocator using a first-fit policy.
///
/// `head` is a dummy node; the first real free region is in `head.next`.
pub struct LinkedListAllocator {
    head: ListNode,
}

impl LinkedListAllocator {
    /// Create a new, empty allocator.
    ///
    /// The allocator does not manage any memory until [`init`] is called.
    pub const fn new() -> Self {
        Self {
            head: ListNode::new(0),
        }
    }

    /// Initialize the allocator with a heap region.
    ///
    /// This inserts the entire heap region as a single free region.
    ///
    /// # Safety
    /// - `heap_start..heap_start + heap_size` must be valid, unused memory.
    /// - Must be called exactly once.
    /// - No allocations may be active when this is called.
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.add_free_region(heap_start, heap_size);
    }

    /// Add a free region to the front of the free list.
    ///
    /// The region is represented by writing a [`ListNode`] at `addr`.
    ///
    /// # Safety
    /// - `addr..addr + size` must be valid memory and unused by anything else.
    /// - `addr` must be aligned for `ListNode`.
    /// - `size` must be at least `size_of::<ListNode>()`.
    unsafe fn add_free_region(&mut self, addr: usize, size: usize) {
        // Ensure that the freed region is capable of holding a ListNode.
        assert_eq!(align_up(addr, mem::align_of::<ListNode>()), addr);
        assert!(size >= mem::size_of::<ListNode>());

        // Write the node into the freed memory and push it to the front.
        let mut node = ListNode::new(size);
        node.next = self.head.next.take();

        let node_ptr = addr as *mut ListNode;
        node_ptr.write(node);
        self.head.next = Some(&mut *node_ptr);
    }

    /// Find the first free region that can satisfy an allocation and remove it
    /// from the list.
    ///
    /// Returns `(region_node, alloc_start_addr)` on success.
    fn find_region(&mut self, size: usize, align: usize) -> Option<(&'static mut ListNode, usize)> {
        // `current` always points to a node whose `next` we might remove.
        let mut current = &mut self.head;

        while let Some(ref mut region) = current.next {
            if let Ok(alloc_start) = Self::alloc_from_region(region, size, align) {
                // Region works: unlink it from the list and return it.
                let next = region.next.take();
                let found = current.next.take().unwrap();
                current.next = next;
                return Some((found, alloc_start));
            }

            // Region doesn't work: advance.
            current = current.next.as_mut().unwrap();
        }

        None
    }

    /// Determine whether `region` can satisfy an allocation of `size` and `align`.
    ///
    /// On success, returns the aligned start address for the allocation.
    ///
    /// Fails if:
    /// - `size` doesn't fit in the region
    /// - alignment pushes the start too far
    /// - splitting would leave a remainder too small to hold a `ListNode`
    fn alloc_from_region(region: &ListNode, size: usize, align: usize) -> Result<usize, ()> {
        let alloc_start = align_up(region.start_addr(), align);
        let alloc_end = alloc_start.checked_add(size).ok_or(())?;

        if alloc_end > region.end_addr() {
            return Err(()); // region too small
        }

        let excess_size = region.end_addr() - alloc_end;
        if excess_size > 0 && excess_size < mem::size_of::<ListNode>() {
            // Splitting would create a trailing free region that can't store a ListNode.
            return Err(());
        }

        Ok(alloc_start)
    }

    /// Adjust `layout` so the allocated region can also store a [`ListNode`].
    ///
    /// Since we write `ListNode` into freed blocks, all allocations must have:
    /// - alignment at least `align_of::<ListNode>()`
    /// - size at least `size_of::<ListNode>()`
    ///
    /// Returns `(adjusted_size, adjusted_align)`.
    fn size_align(layout: Layout) -> (usize, usize) {
        let layout = layout
            .align_to(mem::align_of::<ListNode>())
            .expect("adjusting alignment failed")
            .pad_to_align();

        let size = layout.size().max(mem::size_of::<ListNode>());
        (size, layout.align())
    }
}

unsafe impl GlobalAlloc for Locked<LinkedListAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let (size, align) = LinkedListAllocator::size_align(layout);
        let mut allocator = self.lock();

        if let Some((region, alloc_start)) = allocator.find_region(size, align) {
            let alloc_end = alloc_start.checked_add(size).expect("overflow");
            let excess_size = region.end_addr() - alloc_end;

            // If there's leftover space, add it back as a new free region.
            if excess_size > 0 {
                allocator.add_free_region(alloc_end, excess_size);
            }

            alloc_start as *mut u8
        } else {
            ptr::null_mut()
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let (size, _) = LinkedListAllocator::size_align(layout);
        self.lock().add_free_region(ptr as usize, size);
    }
}
