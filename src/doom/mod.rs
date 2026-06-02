use crate::vga_mode13;
use alloc::alloc::{Layout, alloc, dealloc};

unsafe extern "C" {
    // Call once to initialize Doom (loads WAD, sets up engine state).
    // Blocks until D_DoomMain returns (which it never does normally).
    pub fn doomgeneric_Create(argc: i32, argv: *mut *mut u8);

    // Advance one game tick. Call in a loop at ~35 Hz.
    pub fn doomgeneric_Tick();

    // Doom's output framebuffer. pixel_t = u8 (palette index) because
    // we compile with CMAP256. Points into a malloc'd 320*200 buffer.
    pub static mut DG_ScreenBuffer: *mut u8;
}

// Entry point for the Doom kernel thread.
// Switches to mode 13h, then hands off to doomgeneric. Never returns.
pub fn doom_thread_entry() -> ! {
    // Switching to mode 13h here (not in kernel_main) keeps the text shell
    // alive until Doom is actually ready to take over the display.
    //
    // vga_mode13::init() was already called before spawning this thread;
    // see kernel_main. If not, call it here instead.

    let mut dummy_arg = b"moltos\0".as_ptr() as *mut u8;
    unsafe {
        doomgeneric_Create(1, &mut dummy_arg);
    }
    // doomgeneric_Create calls D_DoomMain which never returns under normal
    // operation. If it does, halt.
    crate::hlt_loop()
}

// ---------------------------------------------------------------------------
// DG_* callbacks — implement these to make Doom actually work.
// All are marked #[unsafe(no_mangle)] so the C linker can find them.
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn DG_Init() {
    // Called once by doomgeneric_Create before D_DoomMain.
    // Good place to load the palette from the WAD if desired;
    // for now the default VGA palette is already in the DAC from vga_mode13.
}

#[unsafe(no_mangle)]
pub extern "C" fn DG_DrawFrame() {
    // Copy Doom's 320×200 palette-index buffer to the VGA framebuffer.
    unsafe {
        if DG_ScreenBuffer.is_null() { return; }
        let src = core::slice::from_raw_parts(DG_ScreenBuffer, vga_mode13::WIDTH * vga_mode13::HEIGHT);
        let dst = vga_mode13::framebuffer();
        dst.copy_from_slice(src);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn DG_SleepMs(_ms: u32) {
    // TODO: yield the thread for approximately _ms milliseconds.
    // For now, spin. Replace with a proper sleep once timing is wired up.
    let target = crate::uptime_ticks() + (_ms as u64 / 10).max(1);
    while crate::uptime_ticks() < target {
        x86_64::instructions::hlt();
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn DG_GetTicksMs() -> u32 {
    // uptime_ticks() is at 100 Hz, so multiply by 10 to get milliseconds.
    (crate::uptime_ticks() * 10) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn DG_GetKey(pressed: *mut i32, key: *mut u8) -> i32 {
    // TODO: drain the PS/2 scancode queue and translate to Doom keycodes.
    // Return 1 if a key event is available, 0 otherwise.
    let _ = (pressed, key);
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn DG_SetWindowTitle(_title: *const u8) {
    // No window title in a kernel. No-op.
}


// implemented for C stubs in stubs.c
#[unsafe(no_mangle)]
pub extern "C" fn rust_alloc(size: usize, align: usize) -> *mut u8 {
    let layout = Layout::from_size_align(size, align).unwrap();
    let ptr = unsafe { alloc(layout) };
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_free(ptr: *mut u8, size: usize, align: usize) {
    let layout = Layout::from_size_align(size, align).unwrap();
    unsafe { dealloc(ptr, layout)}
}
