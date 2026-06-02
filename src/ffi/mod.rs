extern crate alloc;

use alloc::alloc::{Layout, alloc as alloc_fn, dealloc, realloc as realloc_fn};
use core::sync::atomic::{AtomicBool, Ordering};
use crate::vga_mode13;

// ---------------------------------------------------------------------------
// Keyboard state — shared between the interrupt-path scancode consumer and
// DG_GetKey.  We keep a tiny ring buffer of decoded (pressed, doomkey) pairs.
// ---------------------------------------------------------------------------

const KEY_BUF_SIZE: usize = 32;

struct KeyEvent {
    pressed: u8,
    key:     u8,
}

struct KeyRing {
    buf:  [KeyEvent; KEY_BUF_SIZE],
    head: usize,
    tail: usize,
}

impl KeyRing {
    const fn new() -> Self {
        const ZERO: KeyEvent = KeyEvent { pressed: 0, key: 0 };
        Self { buf: [ZERO; KEY_BUF_SIZE], head: 0, tail: 0 }
    }
    fn push(&mut self, ev: KeyEvent) {
        let next = (self.tail + 1) % KEY_BUF_SIZE;
        if next != self.head {
            self.buf[self.tail] = ev;
            self.tail = next;
        }
    }
    fn pop(&mut self) -> Option<KeyEvent> {
        if self.head == self.tail { return None; }
        let ev = KeyEvent { pressed: self.buf[self.head].pressed, key: self.buf[self.head].key };
        self.head = (self.head + 1) % KEY_BUF_SIZE;
        Some(ev)
    }
}

static KEY_RING: spin::Mutex<KeyRing> = spin::Mutex::new(KeyRing::new());
// Tracks whether the previous scancode was the 0xE0 extended-key prefix.
static EXTENDED: AtomicBool = AtomicBool::new(false);

// PS/2 Set-1 make-code → Doom key.  0 = unmapped.
fn scancode_to_doom(sc: u8, extended: bool) -> u8 {
    if extended {
        return match sc {
            0x48 => 0xad,   // up arrow    → KEY_UPARROW
            0x50 => 0xaf,   // down arrow  → KEY_DOWNARROW
            0x4B => 0xac,   // left arrow  → KEY_LEFTARROW
            0x4D => 0xae,   // right arrow → KEY_RIGHTARROW
            0x1D => 0x9d,   // right ctrl  → KEY_RCTRL  (0x80+0x1d)
            0x38 => 0xb8,   // right alt   → KEY_RALT   (0x80+0x38)
            _    => 0,
        };
    }
    match sc {
        0x01 => 27,         // Escape
        0x0E => 0x7f,       // Backspace
        0x0F => 9,          // Tab
        0x1C => 13,         // Enter
        0x39 => b' ',       // Space
        0x1D => 0x9d,       // Left Ctrl   → KEY_RCTRL
        0x2A | 0x36 => 0xb6, // Shift      → KEY_RSHIFT (0x80+0x36)
        0x38 => 0xb8,       // Left Alt    → KEY_RALT
        0x3B => 0xbb,       // F1
        0x3C => 0xbc,       // F2
        0x3D => 0xbd,       // F3
        0x3E => 0xbe,       // F4
        0x3F => 0xbf,       // F5
        0x40 => 0xc0,       // F6
        0x41 => 0xc1,       // F7
        0x42 => 0xc2,       // F8
        0x43 => 0xc3,       // F9
        0x44 => 0xc4,       // F10
        0x57 => 0xd7,       // F11         (0x80+0x57)
        0x58 => 0xd8,       // F12         (0x80+0x58)
        // Row 1: digits
        0x02 => b'1', 0x03 => b'2', 0x04 => b'3', 0x05 => b'4',
        0x06 => b'5', 0x07 => b'6', 0x08 => b'7', 0x09 => b'8',
        0x0A => b'9', 0x0B => b'0',
        0x0C => b'-', 0x0D => b'=',
        // Row 2: QWERTY
        0x10 => b'q', 0x11 => b'w', 0x12 => b'e', 0x13 => b'r',
        0x14 => b't', 0x15 => b'y', 0x16 => b'u', 0x17 => b'i',
        0x18 => b'o', 0x19 => b'p',
        // Row 3: ASDF
        0x1E => b'a', 0x1F => b's', 0x20 => b'd', 0x21 => b'f',
        0x22 => b'g', 0x23 => b'h', 0x24 => b'j', 0x25 => b'k',
        0x26 => b'l',
        // Row 4: ZXCV
        0x2C => b'z', 0x2D => b'x', 0x2E => b'c', 0x2F => b'v',
        0x30 => b'b', 0x31 => b'n', 0x32 => b'm',
        0x33 => b',', 0x34 => b'.', 0x35 => b'/',
        _ => 0,
    }
}

/// Feed a raw PS/2 scancode byte into the Doom key ring.
/// Call this from the keyboard interrupt handler (or anywhere scancodes arrive).
pub fn feed_scancode(sc: u8) {
    // 0xE0 prefix marks an extended key — remember and skip.
    if sc == 0xE0 {
        EXTENDED.store(true, Ordering::Relaxed);
        return;
    }
    let ext = EXTENDED.swap(false, Ordering::Relaxed);

    // Bit 7 = break (key up), bits 6:0 = make code.
    let pressed = (sc & 0x80) == 0;
    let make    = sc & 0x7F;

    let dk = scancode_to_doom(make, ext);
    if dk != 0 {
        x86_64::instructions::interrupts::without_interrupts(|| {
            KEY_RING.lock().push(KeyEvent { pressed: pressed as u8, key: dk });
        });
    }
}

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

// Entry point for the Doom kernel thread. Never returns.
pub fn doom_thread_entry() -> ! {
    // Require doom1.wad to be present in the CXFS archive before starting.
    // Without it, doomgeneric_Create crashes deep in Z_Init / W_InitFiles.
    if crate::fs::open("doom1.wad").is_none() {
        crate::serial_println!("doom: doom1.wad not found in CXFS archive, thread idle");
        crate::hlt_loop();
    }

    // Switch to graphics mode now — text output disappears from this point on.
    crate::vga_mode13::enter_mode13();

    let mut dummy_arg = b"moltos\0".as_ptr() as *mut u8;
    unsafe {
        doomgeneric_Create(1, &mut dummy_arg);
        loop {
            doomgeneric_Tick();
        }
    }
}

// ---------------------------------------------------------------------------
// DG_* callbacks — implement these to make Doom actually work.
// All are marked #[unsafe(no_mangle)] so the C linker can find them.
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn DG_Init() {
    // Load the primary palette from PLAYPAL in doom1.wad and program the VGA DAC.
    // PLAYPAL contains 14 * 256 * 3 bytes; the first 768 bytes are palette 0.
    if let Some(wad) = crate::fs::open("doom1.wad") {
        load_playpal(wad);
    }
}

fn load_playpal(wad: &[u8]) {
    if wad.len() < 12 { return; }
    if &wad[0..4] != b"IWAD" && &wad[0..4] != b"PWAD" { return; }

    let num_lumps  = u32::from_le_bytes(wad[4..8].try_into().unwrap()) as usize;
    let dir_offset = u32::from_le_bytes(wad[8..12].try_into().unwrap()) as usize;

    for i in 0..num_lumps {
        let base = dir_offset + i * 16;
        if base + 16 > wad.len() { break; }
        let entry = &wad[base..base + 16];
        let name  = &entry[8..16];
        let end   = name.iter().position(|&b| b == 0).unwrap_or(8);
        if &name[..end] != b"PLAYPAL" { continue; }

        let lump_off  = u32::from_le_bytes(entry[0..4].try_into().unwrap()) as usize;
        let lump_size = u32::from_le_bytes(entry[4..8].try_into().unwrap()) as usize;
        if lump_size < 768 || lump_off + 768 > wad.len() { return; }

        let palette = &wad[lump_off..lump_off + 768]; // first of 14 palettes
        for (idx, chunk) in palette.chunks_exact(3).enumerate() {
            vga_mode13::set_palette_entry(idx as u8, chunk[0], chunk[1], chunk[2]);
        }
        return;
    }
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
    let ev = x86_64::instructions::interrupts::without_interrupts(|| {
        KEY_RING.lock().pop()
    });
    match ev {
        Some(ev) => {
            unsafe {
                *pressed = ev.pressed as i32;
                *key     = ev.key;
            }
            1
        }
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn DG_SetWindowTitle(_title: *const u8) {
    // No window title in a kernel. No-op.
}


// ---------------------------------------------------------------------------
// Rust functions called from stubs.c
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn rust_alloc(size: usize, align: usize) -> *mut u8 {
    let layout = Layout::from_size_align(size, align).unwrap();
    unsafe { alloc_fn(layout) }
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_free(ptr: *mut u8, size: usize, align: usize) {
    let layout = Layout::from_size_align(size, align).unwrap();
    unsafe { dealloc(ptr, layout) }
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_realloc(ptr: *mut u8, old_size: usize, align: usize, new_size: usize) -> *mut u8 {
    let layout = Layout::from_size_align(old_size, align).unwrap();
    unsafe { realloc_fn(ptr, layout, new_size) }
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_hlt() -> ! {
    crate::hlt_loop()
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_serial_write(s: *const u8, len: usize) {
    let bytes = unsafe { core::slice::from_raw_parts(s, len) };
    if let Ok(s) = core::str::from_utf8(bytes) {
        crate::serial_print!("{}", s);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_fs_open(name: *const u8, out_size: *mut usize) -> *const u8 {
    let name = match unsafe { core::ffi::CStr::from_ptr(name as *const i8) }.to_str() {
        Ok(s) => s,
        Err(_) => return core::ptr::null(),
    };
    match crate::fs::open(name) {
        Some(data) => {
            unsafe { *out_size = data.len(); }
            data.as_ptr()
        }
        None => core::ptr::null(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_uptime_ticks() -> u64 {
    crate::uptime_ticks()
}
