//! Heap initialization + global allocator selection.
//!
//! This module maps a fixed virtual heap region, then initializes the chosen
//! allocator to manage that region.
//!
//! Current heap:
//! - Start: [`HEAP_START`]
//! - Size:  [`HEAP_SIZE`]
//!
//! Allocator in use:
//! - [`FixedSizeBlockAllocator`] wrapped in [`Locked`].

use alloc::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;
use x86_64::{
    structures::paging::{
        mapper::MapToError, FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB,
    },
    VirtAddr,
};

pub mod bump;
pub mod fixed_size_block;
pub mod linked_list;

use bump::BumpAllocator;
use fixed_size_block::FixedSizeBlockAllocator;
use linked_list::LinkedListAllocator;

/// Virtual start address of the heap region.
///
/// This is a fixed virtual address that we map to physical frames during
/// [`init_heap`]. Make sure this region does not overlap your kernel image,
/// page tables, or other reserved mappings.
pub const HEAP_START: usize = 0x_4444_4444_0000;

/// Size of the heap region in bytes.
///
/// This is the total number of bytes mapped and handed to the allocator.
pub const HEAP_SIZE: usize = 100 * 1024; // 100 KiB

/// A very small allocator that always returns null.
///
/// Useful as a placeholder during bring-up, or for testing failure paths.
/// Not used as the global allocator in this file.
pub struct Dummy;

/// A thin wrapper that provides mutual exclusion for allocator types.
///
/// Many allocators are not internally thread-safe; this wrapper provides a
/// `spin::Mutex` so the allocator can be used as a `#[global_allocator]`.
pub struct Locked<A> {
    inner: spin::Mutex<A>,
}

impl<A> Locked<A> {
    /// Wrap `inner` in a `spin::Mutex`.
    pub const fn new(inner: A) -> Self {
        Self {
            inner: spin::Mutex::new(inner),
        }
    }

    /// Acquire the mutex and return a guard for accessing the allocator.
    pub fn lock(&self) -> spin::MutexGuard<'_, A> {
        self.inner.lock()
    }
}

/// The global allocator used by `alloc` types such as `Box`, `Vec`, etc.
///
/// We use the fixed-size block allocator here, protected by a spinlock.
#[global_allocator]
static ALLOCATOR: Locked<FixedSizeBlockAllocator> =
    Locked::new(FixedSizeBlockAllocator::new());

unsafe impl GlobalAlloc for Dummy {
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        null_mut()
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        panic!("dealloc should never be called for Dummy")
    }
}

/// Map the heap region and initialize the global allocator.
///
/// This function:
/// 1. Computes the page range covering [`HEAP_START..HEAP_START+HEAP_SIZE`]
/// 2. Allocates a physical frame per page via `frame_allocator`
/// 3. Maps each page as `PRESENT | WRITABLE` using `mapper`
/// 4. Initializes the selected allocator with the heap region
///
/// # Parameters
/// - `mapper`: A page table mapper used to create virtual→physical mappings.
/// - `frame_allocator`: A physical frame allocator used to obtain frames.
///
/// # Errors
/// Returns [`MapToError::FrameAllocationFailed`] if frames cannot be allocated,
/// or propagates mapping errors from the mapper.
///
/// # Safety / Requirements
/// - Must be called exactly once (or otherwise ensure you don’t double-init the allocator).
/// - The heap virtual region must be unused and valid to map.
/// - `frame_allocator` must return unique, unused frames.
///
/// # Example
/// Call this during early kernel init, before using `Box`, `Vec`, etc.
pub fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE - 1u64;

        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);

        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    for page in page_range {
        let frame = frame_allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;

        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

        unsafe {
            mapper.map_to(page, frame, flags, frame_allocator)?.flush();
        }
    }

    unsafe {
        ALLOCATOR.lock().init(HEAP_START, HEAP_SIZE);
    }

    Ok(())
}

/// Align the given address `addr` upwards to alignment `align`.
///
/// `align` must be a power of two.
///
/// # Returns
/// The smallest address `>= addr` that is a multiple of `align`.
///
/// # Examples
/// - `align_up(0x1003, 0x1000) == 0x2000`
/// - `align_up(0x2000, 0x1000) == 0x2000`
fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}
