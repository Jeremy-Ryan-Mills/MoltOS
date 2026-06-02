use core::arch::naked_asm;
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use x86_64::VirtAddr;
use pic8259::ChainedPics;
use spin;

use crate::gdt;
use crate::println;
use crate::hlt_loop;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: spin::Mutex<ChainedPics> =
    spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();

        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.general_protection_fault.set_handler_fn(gp_fault_handler);
        idt.stack_segment_fault.set_handler_fn(stack_segment_fault_handler);
        idt.segment_not_present.set_handler_fn(segment_not_present_handler);
        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);

        unsafe {
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);

            // Naked-function handlers: registered by raw address because they
            // don't match the extern "x86-interrupt" signature.
            idt[InterruptIndex::Timer.as_usize()]
                .set_handler_addr(VirtAddr::from_ptr(timer_interrupt_entry as *const ()));
            idt[InterruptIndex::Keyboard.as_usize()]
                .set_handler_addr(VirtAddr::from_ptr(keyboard_interrupt_entry as *const ()));
        }

        idt[crate::syscall::SYSCALL_VECTOR as usize].set_handler_fn(crate::syscall::syscall_handler);

        idt
    };
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard,
}

impl InterruptIndex {
    fn as_u8(self) -> u8 { self as u8 }
    fn as_usize(self) -> usize { usize::from(self.as_u8()) }
}

pub fn init_idt() {
    IDT.load();
}

// Configure PIT channel 0 to fire at `frequency` Hz.
pub unsafe fn configure_pit(frequency: u32) {
    use x86_64::instructions::port::Port;
    let divisor = (1193182u32 / frequency) as u16;
    let mut cmd: Port<u8> = Port::new(0x43);
    let mut data: Port<u8> = Port::new(0x40);
    unsafe {
        cmd.write(0x34);
        data.write((divisor & 0xFF) as u8);
        data.write((divisor >> 8) as u8);
    }
}

static TICK_COUNT: AtomicU64 = AtomicU64::new(0);

pub fn uptime_ticks() -> u64 {
    TICK_COUNT.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// IrqFrame — the stack layout built by our naked interrupt entry stubs.
//
// The naked entry pushes all 15 GPRs (r15 first, rax last) immediately on
// entry; the CPU interrupt frame (rip/cs/rflags/rsp/ss) is already above them.
// The resulting contiguous block matches this struct exactly.
// ---------------------------------------------------------------------------
#[repr(C)]
pub struct IrqFrame {
    // GPRs pushed by the naked stub (rax at lowest address = rsp after pushes):
    pub rax:    u64,  // 0
    pub rcx:    u64,  // 8
    pub rdx:    u64,  // 16
    pub rbx:    u64,  // 24
    pub rbp:    u64,  // 32
    pub rsi:    u64,  // 40
    pub rdi:    u64,  // 48
    pub r8:     u64,  // 56
    pub r9:     u64,  // 64
    pub r10:    u64,  // 72
    pub r11:    u64,  // 80
    pub r12:    u64,  // 88
    pub r13:    u64,  // 96
    pub r14:    u64,  // 104
    pub r15:    u64,  // 112
    // CPU-pushed interrupt frame (same privilege, no error code):
    pub rip:    u64,  // 120
    pub cs:     u64,  // 128
    pub rflags: u64,  // 136
    pub rsp:    u64,  // 144
    pub ss:     u64,  // 152
}

// Save the full register state of the interrupted thread into from_ctx,
// send EOI, then switch to to_ctx.  Never returns.
#[inline(always)]
unsafe fn irq_preempt(
    from_ctx: *mut crate::thread::context::ThreadContext,
    to_ctx:   *const crate::thread::context::ThreadContext,
    frame:    *mut IrqFrame,
    irq:      u8,
) {
    unsafe {
        let f   = &*frame;
        let ctx = &mut *from_ctx;

        ctx.rip    = f.rip;
        ctx.rax    = f.rax;
        ctx.rcx    = f.rcx;
        ctx.rdx    = f.rdx;
        ctx.rbx    = f.rbx;
        ctx.rbp    = f.rbp;
        ctx.rsi    = f.rsi;
        ctx.rdi    = f.rdi;
        ctx.r8     = f.r8;
        ctx.r9     = f.r9;
        ctx.r10    = f.r10;
        ctx.r11    = f.r11;
        ctx.r12    = f.r12;
        ctx.r13    = f.r13;
        ctx.r14    = f.r14;
        ctx.r15    = f.r15;
        ctx.rflags = f.rflags;

        // Set up the resume stack so context_switch_to's `ret` pops the right rip.
        // We write rip one slot below the interrupted rsp (like a call instruction
        // would), then point ctx.rsp there.
        let resume_rsp = f.rsp - 8;
        *(resume_rsp as *mut u64) = f.rip;
        ctx.rsp = resume_rsp;

        PICS.lock().notify_end_of_interrupt(irq);
        crate::thread::context::context_switch_to(to_ctx);
    }
}

// ---------------------------------------------------------------------------
// Timer IRQ (IRQ0)
// ---------------------------------------------------------------------------

// Naked entry: save all GPRs, call inner, restore all GPRs, iretq.
// Push order is r15…rax so that rax lands at the lowest address (offset 0 in IrqFrame).
#[unsafe(naked)]
unsafe extern "C" fn timer_interrupt_entry() {
    naked_asm!(
        "push r15", "push r14", "push r13", "push r12",
        "push r11", "push r10", "push r9",  "push r8",
        "push rdi", "push rsi", "push rbp", "push rbx",
        "push rdx", "push rcx", "push rax",
        "mov  rdi, rsp",
        "call {inner}",
        "pop  rax", "pop  rcx", "pop  rdx",
        "pop  rbx", "pop  rbp", "pop  rsi", "pop  rdi",
        "pop  r8",  "pop  r9",  "pop  r10", "pop  r11",
        "pop  r12", "pop  r13", "pop  r14", "pop  r15",
        "iretq",
        inner = sym timer_interrupt_inner,
    );
}

extern "C" fn timer_interrupt_inner(frame: *mut IrqFrame) {
    TICK_COUNT.fetch_add(1, Ordering::Relaxed);

    let switch = {
        let mut sched = crate::thread::SCHEDULER.lock();
        sched.tick_prepare(TICK_COUNT.load(Ordering::Relaxed))
    };

    if let Some((from_ctx, to_ctx)) = switch {
        if core::ptr::eq(from_ctx, to_ctx) {
            unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer.as_u8()); }
            return;
        }
        unsafe {
            irq_preempt(from_ctx, to_ctx, frame, InterruptIndex::Timer.as_u8());
        }
    } else {
        unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer.as_u8()); }
    }
}

// ---------------------------------------------------------------------------
// Keyboard IRQ (IRQ1)
// ---------------------------------------------------------------------------

#[unsafe(naked)]
unsafe extern "C" fn keyboard_interrupt_entry() {
    naked_asm!(
        "push r15", "push r14", "push r13", "push r12",
        "push r11", "push r10", "push r9",  "push r8",
        "push rdi", "push rsi", "push rbp", "push rbx",
        "push rdx", "push rcx", "push rax",
        "mov  rdi, rsp",
        "call {inner}",
        "pop  rax", "pop  rcx", "pop  rdx",
        "pop  rbx", "pop  rbp", "pop  rsi", "pop  rdi",
        "pop  r8",  "pop  r9",  "pop  r10", "pop  r11",
        "pop  r12", "pop  r13", "pop  r14", "pop  r15",
        "iretq",
        inner = sym keyboard_interrupt_inner,
    );
}

extern "C" fn keyboard_interrupt_inner(frame: *mut IrqFrame) {
    let scancode: u8 = unsafe { x86_64::instructions::port::Port::new(0x60u16).read() };
    crate::task::keyboard::add_scancode(scancode);
    crate::ffi::feed_scancode(scancode);

    let switch = {
        let mut sched = crate::thread::SCHEDULER.lock();
        sched.tick_prepare(crate::uptime_ticks())
    };

    if let Some((from_ctx, to_ctx)) = switch {
        if core::ptr::eq(from_ctx, to_ctx) {
            unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8()); }
            return;
        }
        unsafe {
            irq_preempt(from_ctx, to_ctx, frame, InterruptIndex::Keyboard.as_u8());
        }
    } else {
        unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8()); }
    }
}

// ---------------------------------------------------------------------------
// Exception handlers
// ---------------------------------------------------------------------------

extern "x86-interrupt" fn gp_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    crate::serial_println!("EXCEPTION: GENERAL PROTECTION FAULT (code={:#x})", error_code);
    crate::serial_println!("Stack Frame: {:#?}", stack_frame);
    hlt_loop();
}

extern "x86-interrupt" fn stack_segment_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    crate::serial_println!("EXCEPTION: STACK SEGMENT FAULT (code={:#x})", error_code);
    crate::serial_println!("Stack Frame: {:#?}", stack_frame);
    hlt_loop();
}

extern "x86-interrupt" fn segment_not_present_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    crate::serial_println!("EXCEPTION: SEGMENT NOT PRESENT (code={:#x})", error_code);
    crate::serial_println!("Stack Frame: {:#?}", stack_frame);
    hlt_loop();
}

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    crate::serial_println!("EXCEPTION: INVALID OPCODE");
    crate::serial_println!("Stack Frame: {:#?}", stack_frame);
    hlt_loop();
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;
    crate::serial_println!("EXCEPTION: PAGE FAULT");
    crate::serial_println!("Accessed Address: {:?}", Cr2::read());
    crate::serial_println!("Error Code: {:?}", error_code);
    crate::serial_println!("Stack Frame: {:#?}", stack_frame);
    hlt_loop();
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    crate::serial_println!("EXCEPTION: DOUBLE FAULT");
    crate::serial_println!("Stack Frame: {:#?}", stack_frame);
    hlt_loop();
}

#[test_case]
fn test_breakpoint_exception() {
    x86_64::instructions::interrupts::int3();
}

#[test_case]
fn test_uptime_increments() {
    if !x86_64::instructions::interrupts::are_enabled() {
        x86_64::instructions::interrupts::enable();
    }
    let t0 = crate::uptime_ticks();
    let mut iterations = 0u32;
    while crate::uptime_ticks() == t0 && iterations < 10_000_000 {
        x86_64::instructions::hlt();
        iterations += 1;
    }
    let t1 = crate::uptime_ticks();
    assert!(t1 > t0, "uptime did not increase ({} -> {})", t0, t1);
    assert!(iterations < 10_000_000, "test timed out waiting for timer interrupt");
}

// IrqFrame layout tests.
//
// The naked interrupt entry stubs push GPRs in a fixed order so that
// irq_preempt can read them by name from the struct.  Any reordering would
// silently corrupt the saved register state — these tests catch that.
#[test_case]
fn test_irq_frame_gpr_offsets() {
    use core::mem::offset_of;
    assert_eq!(offset_of!(IrqFrame, rax),    0);
    assert_eq!(offset_of!(IrqFrame, rcx),    8);
    assert_eq!(offset_of!(IrqFrame, rdx),    16);
    assert_eq!(offset_of!(IrqFrame, rbx),    24);
    assert_eq!(offset_of!(IrqFrame, rbp),    32);
    assert_eq!(offset_of!(IrqFrame, rsi),    40);
    assert_eq!(offset_of!(IrqFrame, rdi),    48);
    assert_eq!(offset_of!(IrqFrame, r8),     56);
    assert_eq!(offset_of!(IrqFrame, r9),     64);
    assert_eq!(offset_of!(IrqFrame, r10),    72);
    assert_eq!(offset_of!(IrqFrame, r11),    80);
    assert_eq!(offset_of!(IrqFrame, r12),    88);
    assert_eq!(offset_of!(IrqFrame, r13),    96);
    assert_eq!(offset_of!(IrqFrame, r14),    104);
    assert_eq!(offset_of!(IrqFrame, r15),    112);
}

// The CPU-pushed interrupt frame sits immediately after the 15 GPRs.
#[test_case]
fn test_irq_frame_cpu_frame_offsets() {
    use core::mem::offset_of;
    assert_eq!(offset_of!(IrqFrame, rip),    120);
    assert_eq!(offset_of!(IrqFrame, cs),     128);
    assert_eq!(offset_of!(IrqFrame, rflags), 136);
    assert_eq!(offset_of!(IrqFrame, rsp),    144);
    assert_eq!(offset_of!(IrqFrame, ss),     152);
}

// Total size must be exactly 160 bytes: 15 GPRs + 5 CPU-frame words.
#[test_case]
fn test_irq_frame_size() {
    assert_eq!(core::mem::size_of::<IrqFrame>(), 160);
}

// The rflags saved in the CPU interrupt frame always has IF=1 because the
// CPU saves the pre-interrupt rflags (when the thread was running) and the
// thread runs with interrupts enabled.  Verify our understanding of the
// IF bit position in the field we read in irq_preempt.
#[test_case]
fn test_irq_frame_rflags_if_bit_position() {
    // Construct a synthetic IrqFrame with a known rflags value and verify
    // the IF bit (bit 9 = 0x200) is where irq_preempt expects it.
    let mut frame = IrqFrame {
        rax: 0, rcx: 0, rdx: 0, rbx: 0, rbp: 0, rsi: 0, rdi: 0,
        r8:  0, r9:  0, r10: 0, r11: 0, r12: 0, r13: 0, r14: 0, r15: 0,
        rip: 0x1234, cs: 8, rflags: 0x246, rsp: 0xffff_8000_0000_0000, ss: 0,
    };
    assert_ne!(frame.rflags & 0x200, 0, "IF bit (0x200) not set in rflags=0x246");
    frame.rflags &= !0x200u64;
    assert_eq!(frame.rflags & 0x200, 0, "IF bit clear after mask");
}

// uptime_ticks must be monotonically non-decreasing across consecutive reads.
#[test_case]
fn test_uptime_monotonic() {
    let t0 = uptime_ticks();
    let t1 = uptime_ticks();
    assert!(t1 >= t0, "uptime went backwards: {} -> {}", t0, t1);
}

// PIC offsets: IRQ0/IRQ1 must map to the remapped vectors, not the default
// (conflicting) ones.  Wrong offsets would shadow CPU exceptions.
#[test_case]
fn test_pic_offsets_clear_of_exceptions() {
    // x86 exceptions use vectors 0-31.  Our remapped PIC starts at PIC_1_OFFSET.
    assert!(PIC_1_OFFSET >= 32, "PIC_1 overlaps CPU exception vectors");
    assert!(PIC_2_OFFSET >= 40, "PIC_2 overlaps CPU exception vectors");
    assert_eq!(PIC_2_OFFSET, PIC_1_OFFSET + 8);
}

// Timer and keyboard IRQ vectors must be distinct and within the PIC range.
#[test_case]
fn test_irq_vectors_in_pic_range() {
    let timer = InterruptIndex::Timer.as_usize();
    let kb    = InterruptIndex::Keyboard.as_usize();
    assert_eq!(timer, PIC_1_OFFSET as usize);
    assert_eq!(kb,    PIC_1_OFFSET as usize + 1);
    assert_ne!(timer, kb);
}
