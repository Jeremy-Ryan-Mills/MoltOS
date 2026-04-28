/***
 * src/interrupts.rs
 *
 * IDT setup, PIC configuration, and hardware IRQ handlers.
 * Timer (IRQ0) and keyboard (IRQ1) both trigger the scheduler on each interrupt.
 */

use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
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

        unsafe {
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }

        idt[InterruptIndex::Timer.as_usize()].set_handler_fn(timer_interrupt_handler);
        idt[InterruptIndex::Keyboard.as_usize()].set_handler_fn(keyboard_interrupt_handler);
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
// PIT base clock is 1.193182 MHz; divisor = base / frequency.
pub unsafe fn configure_pit(frequency: u32) {
    use x86_64::instructions::port::Port;
    let divisor = (1193182u32 / frequency) as u16;
    let mut cmd: Port<u8> = Port::new(0x43);
    let mut data: Port<u8> = Port::new(0x40);
    unsafe {
        cmd.write(0x34); // channel 0, lobyte/hibyte, mode 2 (rate generator), binary
        data.write((divisor & 0xFF) as u8);
        data.write((divisor >> 8) as u8);
    }
}

static TICK_COUNT: AtomicU64 = AtomicU64::new(0);

pub fn uptime_ticks() -> u64 {
    TICK_COUNT.load(Ordering::Relaxed)
}

// Write the interrupted thread's state into from_ctx, send EOI, then jump to to_ctx.
// Called from interrupt handlers after they've saved the callee-saved registers.
// Does not return — execution continues in to_ctx's thread.
#[inline(always)]
unsafe fn irq_preempt(
    from_ctx: *mut crate::thread::context::ThreadContext,
    to_ctx: *const crate::thread::context::ThreadContext,
    rip: u64,
    rsp: u64,
    saved: [u64; 6],  // rbx, rbp, r12, r13, r14, r15
    irq: u8,
) {
    unsafe {
        let ctx = &mut *from_ctx;
        ctx.rip = rip;
        ctx.rsp = rsp;
        ctx.rbx = saved[0];
        ctx.rbp = saved[1];
        ctx.r12 = saved[2];
        ctx.r13 = saved[3];
        ctx.r14 = saved[4];
        ctx.r15 = saved[5];
        PICS.lock().notify_end_of_interrupt(irq);
        crate::thread::context::context_switch_to(to_ctx);
    }
}

// Timer IRQ (IRQ0): increment tick counter, trigger scheduler, optionally switch threads.
extern "x86-interrupt" fn timer_interrupt_handler(stack_frame: InterruptStackFrame) {
    // Capture callee-saved regs immediately — before any Rust code runs that
    // could clobber them. These hold the interrupted thread's register state.
    let saved_rbx: u64; let saved_rbp: u64;
    let saved_r12: u64; let saved_r13: u64;
    let saved_r14: u64; let saved_r15: u64;
    unsafe {
        core::arch::asm!(
            "mov {}, rbx", "mov {}, rbp",
            "mov {}, r12", "mov {}, r13",
            "mov {}, r14", "mov {}, r15",
            out(reg) saved_rbx, out(reg) saved_rbp,
            out(reg) saved_r12, out(reg) saved_r13,
            out(reg) saved_r14, out(reg) saved_r15,
        );
    }

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
            irq_preempt(
                from_ctx, to_ctx,
                stack_frame.instruction_pointer.as_u64(),
                stack_frame.stack_pointer.as_u64(),
                [saved_rbx, saved_rbp, saved_r12, saved_r13, saved_r14, saved_r15],
                InterruptIndex::Timer.as_u8(),
            );
        }
    } else {
        unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer.as_u8()); }
    }
}

// Keyboard IRQ (IRQ1): read scancode, enqueue it, trigger scheduler, optionally switch.
extern "x86-interrupt" fn keyboard_interrupt_handler(stack_frame: InterruptStackFrame) {
    // Capture callee-saved regs first — same reason as timer handler above.
    let saved_rbx: u64; let saved_rbp: u64;
    let saved_r12: u64; let saved_r13: u64;
    let saved_r14: u64; let saved_r15: u64;
    unsafe {
        core::arch::asm!(
            "mov {}, rbx", "mov {}, rbp",
            "mov {}, r12", "mov {}, r13",
            "mov {}, r14", "mov {}, r15",
            out(reg) saved_rbx, out(reg) saved_rbp,
            out(reg) saved_r12, out(reg) saved_r13,
            out(reg) saved_r14, out(reg) saved_r15,
        );
    }

    let scancode: u8 = unsafe { x86_64::instructions::port::Port::new(0x60u16).read() };
    crate::task::keyboard::add_scancode(scancode);

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
            irq_preempt(
                from_ctx, to_ctx,
                stack_frame.instruction_pointer.as_u64(),
                stack_frame.stack_pointer.as_u64(),
                [saved_rbx, saved_rbp, saved_r12, saved_r13, saved_r14, saved_r15],
                InterruptIndex::Keyboard.as_u8(),
            );
        }
    } else {
        unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8()); }
    }
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
