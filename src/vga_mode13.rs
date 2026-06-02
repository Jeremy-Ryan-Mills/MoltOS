use x86_64::{
    instructions::port::Port,
    structures::paging::{
        mapper::MapToError, FrameAllocator, Mapper, Page, PageTableFlags, PhysFrame, Size4KiB,
    },
    PhysAddr, VirtAddr,
};

pub const WIDTH:   usize = 320;
pub const HEIGHT:  usize = 200;
pub const FB_ADDR: usize = 0xA0000;

const MISC_WRITE:    u16 = 0x3C2;
const SEQ_INDEX:     u16 = 0x3C4;
const SEQ_DATA:      u16 = 0x3C5;
const CRTC_INDEX:    u16 = 0x3D4;
const CRTC_DATA:     u16 = 0x3D5;
const GC_INDEX:      u16 = 0x3CE;
const GC_DATA:       u16 = 0x3CF;
const AC_PORT:       u16 = 0x3C0;
const INPUT_STATUS1: u16 = 0x3DA; // read resets AC address/data flip-flop
const DAC_INDEX:     u16 = 0x3C8;
const DAC_DATA:      u16 = 0x3C9;

unsafe fn write_indexed(index_port: u16, data_port: u16, regs: &[(u8, u8)]) {
    let mut idx: Port<u8> = Port::new(index_port);
    let mut dat: Port<u8> = Port::new(data_port);
    for &(i, v) in regs {
        unsafe { idx.write(i); dat.write(v); }
    }
}

/// Map the VGA framebuffer pages into virtual memory. Call this at boot while
/// the mapper is available. Does not switch the display to graphics mode.
pub fn init(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) {
    map_framebuffer(mapper, frame_allocator);
}

/// Switch the display to VGA mode 13h. Call this immediately before starting
/// the Doom engine — this is when text output stops being visible.
pub fn enter_mode13() {
    set_mode();
    framebuffer().fill(0);
}

fn set_mode() {
    unsafe {
        Port::<u8>::new(MISC_WRITE).write(0x63);

        write_indexed(SEQ_INDEX, SEQ_DATA, &[
            (0x00, 0x03), // Reset: run
            (0x01, 0x01), // Clocking Mode: 8-dot chars
            (0x02, 0x0F), // Map Mask: all planes
            (0x03, 0x00), // Character Map Select
            (0x04, 0x0E), // Memory Mode: chain-4, extended
        ]);

        // Unlock CRTC registers (CR11 bit 7 protects CR00-CR07).
        let mut crtc_idx: Port<u8> = Port::new(CRTC_INDEX);
        let mut crtc_dat: Port<u8> = Port::new(CRTC_DATA);
        crtc_idx.write(0x11);
        let cr11 = crtc_dat.read() & !0x80;
        crtc_dat.write(cr11);

        write_indexed(CRTC_INDEX, CRTC_DATA, &[
            (0x00, 0x5F), (0x01, 0x4F), (0x02, 0x50), (0x03, 0x82),
            (0x04, 0x54), (0x05, 0x80), (0x06, 0xBF), (0x07, 0x1F),
            (0x08, 0x00), (0x09, 0x41), (0x0A, 0x00), (0x0B, 0x00),
            (0x0C, 0x00), (0x0D, 0x00), (0x0E, 0x00), (0x0F, 0x00),
            (0x10, 0x9C), (0x11, 0x0E), (0x12, 0x8F), (0x13, 0x28),
            (0x14, 0x40), (0x15, 0x96), (0x16, 0xB9), (0x17, 0xA3),
            (0x18, 0xFF),
        ]);

        write_indexed(GC_INDEX, GC_DATA, &[
            (0x00, 0x00), (0x01, 0x00), (0x02, 0x00), (0x03, 0x00),
            (0x04, 0x00), (0x05, 0x40), (0x06, 0x05), (0x07, 0x0F),
            (0x08, 0xFF),
        ]);

        // AC: reset flip-flop by reading INPUT_STATUS1, then write index+data
        // alternately to 0x3C0.  Finish with 0x20 to re-enable display output.
        let _: u8 = Port::<u8>::new(INPUT_STATUS1).read();
        let mut ac: Port<u8> = Port::new(AC_PORT);
        for (i, v) in (0x00u8..=0x0Fu8).map(|i| (i, i)) {
            ac.write(i); ac.write(v); // palette: identity map
        }
        ac.write(0x10); ac.write(0x41); // Mode Control
        ac.write(0x11); ac.write(0x00); // Overscan Color
        ac.write(0x12); ac.write(0x0F); // Color Plane Enable
        ac.write(0x13); ac.write(0x00); // Horizontal Pixel Pan
        ac.write(0x14); ac.write(0x00); // Color Select
        ac.write(0x20); // enable display
    }
}

// Identity-map the 64 KiB framebuffer window (0xA0000-0xAFFFF, 16 pages).
fn map_framebuffer(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) {
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
    for i in 0..16u64 {
        let phys = PhysAddr::new(FB_ADDR as u64 + i * 4096);
        let page  = Page::<Size4KiB>::containing_address(VirtAddr::new(phys.as_u64()));
        let frame = PhysFrame::<Size4KiB>::containing_address(phys);
        match unsafe { mapper.map_to(page, frame, flags, frame_allocator) } {
            Ok(flush) => flush.flush(),
            Err(MapToError::PageAlreadyMapped(_)) => {}
            Err(e) => panic!("mode13 map failed: {:?}", e),
        }
    }
}

pub fn framebuffer() -> &'static mut [u8; WIDTH * HEIGHT] {
    unsafe { &mut *(FB_ADDR as *mut [u8; WIDTH * HEIGHT]) }
}

// Set a single DAC palette entry. r/g/b are 8-bit; the VGA DAC is 6-bit,
// so values are shifted right by 2.
pub fn set_palette_entry(index: u8, r: u8, g: u8, b: u8) {
    unsafe {
        Port::<u8>::new(DAC_INDEX).write(index);
        Port::<u8>::new(DAC_DATA).write(r >> 2);
        Port::<u8>::new(DAC_DATA).write(g >> 2);
        Port::<u8>::new(DAC_DATA).write(b >> 2);
    }
}
