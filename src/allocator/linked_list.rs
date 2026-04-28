// First-fit free-list allocator. Each free region starts with a ListNode
// written in-place. Allocations walk the list and split regions.
// No coalescing — can fragment over time. Good as a fallback or reference impl.

use alloc::alloc::{GlobalAlloc, Layout};
use core::{mem, ptr};
use super::{align_up, Locked};

struct ListNode {
    size: usize,
    next: Option<&'static mut ListNode>,
}

impl ListNode {
    const fn new(size: usize) -> Self { Self { size, next: None } }
    fn start_addr(&self) -> usize { self as *const Self as usize }
    fn end_addr(&self) -> usize { self.start_addr() + self.size }
}

pub struct LinkedListAllocator {
    head: ListNode,
}

impl LinkedListAllocator {
    pub const fn new() -> Self {
        Self { head: ListNode::new(0) }
    }

    // Safety: heap_start..heap_start+heap_size must be valid unused memory.
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        unsafe { self.add_free_region(heap_start, heap_size); }
    }

    unsafe fn add_free_region(&mut self, addr: usize, size: usize) {
        assert_eq!(align_up(addr, mem::align_of::<ListNode>()), addr);
        assert!(size >= mem::size_of::<ListNode>());
        let mut node = ListNode::new(size);
        node.next = self.head.next.take();
        let node_ptr = addr as *mut ListNode;
        unsafe {
            node_ptr.write(node);
            self.head.next = Some(&mut *node_ptr);
        }
    }

    fn find_region(&mut self, size: usize, align: usize) -> Option<(&'static mut ListNode, usize)> {
        let mut current = &mut self.head;
        while let Some(ref mut region) = current.next {
            if let Ok(alloc_start) = Self::alloc_from_region(region, size, align) {
                let next = region.next.take();
                let found = current.next.take().unwrap();
                current.next = next;
                return Some((found, alloc_start));
            }
            current = current.next.as_mut().unwrap();
        }
        None
    }

    fn alloc_from_region(region: &ListNode, size: usize, align: usize) -> Result<usize, ()> {
        let alloc_start = align_up(region.start_addr(), align);
        let alloc_end = alloc_start.checked_add(size).ok_or(())?;
        if alloc_end > region.end_addr() { return Err(()); }
        let excess = region.end_addr() - alloc_end;
        if excess > 0 && excess < mem::size_of::<ListNode>() { return Err(()); }
        Ok(alloc_start)
    }

    // Round up size/align so freed blocks can hold a ListNode.
    fn size_align(layout: Layout) -> (usize, usize) {
        let layout = layout
            .align_to(mem::align_of::<ListNode>()).expect("alignment failed")
            .pad_to_align();
        (layout.size().max(mem::size_of::<ListNode>()), layout.align())
    }
}

unsafe impl GlobalAlloc for Locked<LinkedListAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let (size, align) = LinkedListAllocator::size_align(layout);
        let mut allocator = self.lock();
        if let Some((region, alloc_start)) = allocator.find_region(size, align) {
            let alloc_end = alloc_start.checked_add(size).expect("overflow");
            let excess = region.end_addr() - alloc_end;
            if excess > 0 {
                unsafe { allocator.add_free_region(alloc_end, excess); }
            }
            alloc_start as *mut u8
        } else {
            ptr::null_mut()
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let (size, _) = LinkedListAllocator::size_align(layout);
        unsafe { self.lock().add_free_region(ptr as usize, size); }
    }
}
