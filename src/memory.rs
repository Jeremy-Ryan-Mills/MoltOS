/***
 * src/memory.rs
 *
 * Page table setup, physical frame allocator, and VGA buffer mapping.
 */

use conquer_once::spin::OnceCell;
use x86_64::{
    structures::paging::{
        mapper::MapToError, Page, PhysFrame, Mapper, Size4KiB, FrameAllocator, PageTable,
        OffsetPageTable,
    },
    VirtAddr, PhysAddr,
};
use bootloader::bootinfo::{MemoryMap, MemoryRegionType};

static MEMORY_MAP: OnceCell<&'static MemoryMap> = OnceCell::uninit();

pub fn init_memory_map(map: &'static MemoryMap) {
    MEMORY_MAP.try_init_once(|| map).expect("init_memory_map called more than once");
}

pub fn dump_memory_map() {
    let map = MEMORY_MAP.try_get().expect("memory map not initialized");
    for region in map.iter() {
        let start = region.range.start_addr();
        let end = region.range.end_addr();
        let size_kib = (end - start) / 1024;
        let kind = match region.region_type {
            MemoryRegionType::Usable => "Usable",
            MemoryRegionType::Reserved => "Reserved",
            _ => "Other",
        };
        crate::println!("  {:016x}-{:016x} {:>8} KiB  {}", start, end, size_kib, kind);
    }
}

pub struct EmptyFrameAllocator;

pub struct BootInfoFrameAllocator {
    memory_map: &'static MemoryMap,
    next: usize,
}

unsafe impl FrameAllocator<Size4KiB> for EmptyFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> { None }
}

impl BootInfoFrameAllocator {
    // Safety: caller must ensure all USABLE frames in the map are truly unused.
    pub unsafe fn init(memory_map: &'static MemoryMap) -> Self {
        BootInfoFrameAllocator { memory_map, next: 0 }
    }

    fn usable_frames(&self) -> impl Iterator<Item = PhysFrame> {
        let addr_ranges = self.memory_map.iter()
            .filter(|r| r.region_type == MemoryRegionType::Usable)
            .map(|r| r.range.start_addr()..r.range.end_addr());
        addr_ranges
            .flat_map(|r| r.step_by(4096))
            .map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        let frame = self.usable_frames().nth(self.next);
        self.next += 1;
        frame
    }
}

// Safety: physical memory must be fully mapped at physical_memory_offset.
// Call only once to avoid aliasing mutable references.
pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    unsafe {
        let level_4_table = active_level_4_table(physical_memory_offset);
        OffsetPageTable::new(level_4_table, physical_memory_offset)
    }
}

unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    use x86_64::registers::control::Cr3;
    let (level_4_table_frame, _) = Cr3::read();
    let phys = level_4_table_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    unsafe { &mut *virt.as_mut_ptr::<PageTable>() }
}

// Identity-map the VGA text buffer so virtual address 0xb8000 is valid.
// Treats PageAlreadyMapped as success (bootloader may have done it already).
pub fn map_vga_buffer(
    mapper: &mut OffsetPageTable,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) {
    use x86_64::structures::paging::PageTableFlags as Flags;

    let page = Page::<Size4KiB>::containing_address(VirtAddr::new(0xb8000));
    let frame = PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(0xb8000));
    let flags = Flags::PRESENT | Flags::WRITABLE;

    match unsafe { mapper.map_to(page, frame, flags, frame_allocator) } {
        Ok(flush) => flush.flush(),
        Err(MapToError::PageAlreadyMapped(_)) => {}
        Err(e) => panic!("VGA map_to failed: {:?}", e),
    }
}

// Translate a virtual address to its physical address.
// Returns None if the address is not mapped.
// Safety: physical memory must be fully mapped at physical_memory_offset.
pub unsafe fn translate_addr(addr: VirtAddr, physical_memory_offset: VirtAddr) -> Option<PhysAddr> {
    use x86_64::structures::paging::page_table::FrameError;
    use x86_64::registers::control::Cr3;

    let (level_4_table_frame, _) = Cr3::read();
    let table_indexes = [addr.p4_index(), addr.p3_index(), addr.p2_index(), addr.p1_index()];
    let mut frame = level_4_table_frame;

    for &index in &table_indexes {
        let virt = physical_memory_offset + frame.start_address().as_u64();
        let table = unsafe { &*virt.as_ptr::<PageTable>() };
        frame = match table[index].frame() {
            Ok(frame) => frame,
            Err(FrameError::FrameNotPresent) => return None,
            Err(FrameError::HugeFrame) => panic!("huge pages not supported"),
        };
    }

    Some(frame.start_address() + u64::from(addr.page_offset()))
}
