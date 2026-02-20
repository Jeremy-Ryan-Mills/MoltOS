//! Interrupt and exception setup.
//!
//! This module builds and loads the CPU’s IDT (Interrupt Descriptor Table),
//! sets up handlers for a few exceptions, and wires up PIC-based hardware IRQs
//! (timer + keyboard). It also provides a small enum for mapping IRQ lines to
//! IDT vector indices.

use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use pic8259::ChainedPics;
use spin;

use crate::gdt;
use crate::println;
use crate::hlt_loop;

/// Offset where PIC1 vectors start in the IDT.
///
/// On x86, vectors 0–31 are reserved for CPU exceptions. Remapping the PICs to
/// start at 32 avoids collisions with those exceptions.
pub const PIC_1_OFFSET: u8 = 32;

/// Offset where PIC2 vectors start in the IDT.
///
/// PIC2 is chained behind PIC1 and occupies the next 8 vectors.
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

/// Global handle to the legacy 8259 PICs.
///
/// This is protected by a spinlock because handlers can run at interrupt time.
/// Access is `unsafe` internally because the PICs are a global piece of hardware
/// with side effects.
pub static PICS: spin::Mutex<ChainedPics> =
    spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

lazy_static! {
    /// The system Interrupt Descriptor Table.
    ///
    /// Built once at runtime and then loaded with [`init_idt`]. We install:
    /// - breakpoint exception handler
    /// - double-fault handler on a dedicated IST stack
    /// - PIC timer and keyboard IRQ handlers
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();

        // CPU exceptions
        idt.breakpoint.set_handler_fn(breakpoint_handler);

        // Page faults
        idt.page_fault.set_handler_fn(page_fault_handler);

        // Double fault: use a known-good stack (IST) so stack overflows don't
        // immediately cascade into triple faults / resets.
        unsafe {
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }

        // Hardware IRQs from the remapped PICs
        idt[InterruptIndex::Timer.as_usize()]
            .set_handler_fn(timer_interrupt_handler);

        idt[InterruptIndex::Keyboard.as_usize()]
            .set_handler_fn(keyboard_interrupt_handler);

        // Syscall gate (int 0x80)
        idt[crate::syscall::SYSCALL_VECTOR as usize]
            .set_handler_fn(crate::syscall::syscall_handler);

        idt
    };
}

/// IDT vector numbers for PIC-delivered hardware interrupts.
///
/// We remap the PIC so that IRQ0 (timer) starts at [`PIC_1_OFFSET`], then assign
/// sequential vectors from there.
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    /// IRQ0: PIT timer interrupt.
    Timer = PIC_1_OFFSET,
    /// IRQ1: PS/2 keyboard interrupt.
    Keyboard,
}

impl InterruptIndex {
    /// Return this interrupt’s IDT vector number as a `u8`.
    fn as_u8(self) -> u8 {
        self as u8
    }

    /// Return this interrupt’s IDT vector number as a `usize` for indexing.
    fn as_usize(self) -> usize {
        usize::from(self.as_u8())
    }
}

/// Configure the PIT (Programmable Interval Timer) to fire at the specified frequency.
///
/// The PIT runs at 1.193182 MHz. To get `frequency` Hz, we use divisor = 1193182 / frequency.
/// Common frequencies:
/// - 18.2 Hz: default (divisor 65536)
/// - 100 Hz: responsive (divisor 11932)
/// - 1000 Hz: very responsive (divisor 1193)
pub unsafe fn configure_pit(frequency: u32) {
    use x86_64::instructions::port::Port;
    
    let divisor = (1193182u32 / frequency) as u16;
    
    // Channel 0, mode 2 (rate generator), 16-bit binary
    let mut command_port = Port::new(0x43);
    command_port.write(0x34u8); // Channel 0, access mode: low byte then high byte, mode 2, binary
    
    // Write divisor (low byte, then high byte)
    let mut data_port = Port::new(0x40);
    data_port.write((divisor & 0xFF) as u8);
    data_port.write((divisor >> 8) as u8);
}

/// Load the IDT into the CPU.
///
/// Call this during early boot after the GDT/TSS is set up.
pub fn init_idt() {
    IDT.load();
}

/// Number of PIT timer ticks since boot. Incremented by the timer interrupt.
static TICK_COUNT: AtomicU64 = AtomicU64::new(0);

/// Returns the number of timer ticks since boot (uptime in ticks).
pub fn uptime_ticks() -> u64 {
    TICK_COUNT.load(Ordering::Relaxed)
}

/// Timer IRQ handler (PIT, IRQ0).
///
/// Increments the tick count, may trigger a thread context switch, then sends EOI.
/// When switching, we save the *interrupted* thread's state (from the stack frame
/// and callee-saved regs) into its context, then switch to the new thread. We must
/// not save the handler's own rsp/rip or we would corrupt the thread's context.
extern "x86-interrupt" fn timer_interrupt_handler(
    stack_frame: InterruptStackFrame)
{
    // Save callee-saved regs immediately; they still hold the interrupted thread's values.
    let saved_rbx: u64;
    let saved_rbp: u64;
    let saved_r12: u64;
    let saved_r13: u64;
    let saved_r14: u64;
    let saved_r15: u64;
    unsafe {
        core::arch::asm!(
            "mov {}, rbx",
            "mov {}, rbp",
            "mov {}, r12",
            "mov {}, r13",
            "mov {}, r14",
            "mov {}, r15",
            out(reg) saved_rbx,
            out(reg) saved_rbp,
            out(reg) saved_r12,
            out(reg) saved_r13,
            out(reg) saved_r14,
            out(reg) saved_r15,
        );
    }

    TICK_COUNT.fetch_add(1, Ordering::Relaxed);

    let switch = {
        let mut sched = crate::thread::SCHEDULER.lock();
        let current_tick = TICK_COUNT.load(Ordering::Relaxed);
        sched.tick_prepare(current_tick)
    };

    if let Some((from_ctx, to_ctx)) = switch {
        // Same-thread "switch" is a no-op; just EOI and return.
        if core::ptr::eq(from_ctx, to_ctx) {
            unsafe {
                PICS.lock()
                    .notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
            }
            return;
        }
        unsafe {
            // Write the *interrupted* thread's state into from_ctx, then load to_ctx.
            (*from_ctx).rsp = stack_frame.stack_pointer.as_u64();
            (*from_ctx).rip = stack_frame.instruction_pointer.as_u64();
            (*from_ctx).rbx = saved_rbx;
            (*from_ctx).rbp = saved_rbp;
            (*from_ctx).r12 = saved_r12;
            (*from_ctx).r13 = saved_r13;
            (*from_ctx).r14 = saved_r14;
            (*from_ctx).r15 = saved_r15;
            // EOI before switching so the interrupt is acknowledged; we won't return.
            PICS.lock().notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
            crate::thread::context::context_switch_to(to_ctx);
        }
        // If we switched, we never return here (we're in another thread now).
    } else {
        unsafe {
            PICS.lock()
                .notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
        }
    }
}

/// Keyboard IRQ handler (PS/2, IRQ1).
///
/// Reads a scancode from port `0x60`, feeds it into the `pc_keyboard` decoder,
/// and prints either the decoded Unicode character or the raw key value.
/// Also triggers a scheduler tick so threads can switch even without timer interrupts.
/// Finally, sends an EOI to the PIC.
extern "x86-interrupt" fn keyboard_interrupt_handler(
    stack_frame: InterruptStackFrame)
{
    use x86_64::instructions::port::Port;

    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };
    crate::task::keyboard::add_scancode(scancode);

    // Trigger scheduler tick so threads can switch even without PIT configured
    // This allows the executor thread to get CPU time when keyboard input arrives
    let switch = {
        let mut sched = crate::thread::SCHEDULER.lock();
        let current_tick = crate::uptime_ticks();
        sched.tick_prepare(current_tick)
    };

    if let Some((from_ctx, to_ctx)) = switch {
        // Same-thread "switch" is a no-op and can corrupt state if we save/restore;
        // just EOI and return so the interrupted thread continues.
        if core::ptr::eq(from_ctx, to_ctx) {
            unsafe {
                PICS.lock()
                    .notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
            }
            return;
        }
        // Save callee-saved regs (same as timer interrupt handler)
        let saved_rbx: u64;
        let saved_rbp: u64;
        let saved_r12: u64;
        let saved_r13: u64;
        let saved_r14: u64;
        let saved_r15: u64;
        unsafe {
            core::arch::asm!(
                "mov {}, rbx",
                "mov {}, rbp",
                "mov {}, r12",
                "mov {}, r13",
                "mov {}, r14",
                "mov {}, r15",
                out(reg) saved_rbx,
                out(reg) saved_rbp,
                out(reg) saved_r12,
                out(reg) saved_r13,
                out(reg) saved_r14,
                out(reg) saved_r15,
            );
        }
        
        unsafe {
            // Write the interrupted thread's state into from_ctx, then load to_ctx
            (*from_ctx).rsp = stack_frame.stack_pointer.as_u64();
            (*from_ctx).rip = stack_frame.instruction_pointer.as_u64();
            (*from_ctx).rbx = saved_rbx;
            (*from_ctx).rbp = saved_rbp;
            (*from_ctx).r12 = saved_r12;
            (*from_ctx).r13 = saved_r13;
            (*from_ctx).r14 = saved_r14;
            (*from_ctx).r15 = saved_r15;
            // EOI before switching
            PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
            crate::thread::context::context_switch_to(to_ctx);
        }
        // If we switched, we never return here
    } else {
        unsafe {
            PICS.lock()
                .notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
        }
    }
}

/// Breakpoint exception handler (INT3).
///
/// Useful for testing that the IDT is loaded correctly and exceptions are
/// reaching Rust handlers.
extern "x86-interrupt" fn breakpoint_handler(
    stack_frame: InterruptStackFrame)
{
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;

    // Use serial output instead of VGA to avoid double fault if VGA isn't mapped.
    crate::serial_println!("EXCEPTION: PAGE FAULT");
    crate::serial_println!("Accessed Address: {:?}", Cr2::read());
    crate::serial_println!("Error Code: {:?}", error_code);
    crate::serial_println!("Stack Frame: {:#?}", stack_frame);
    hlt_loop();
}

/// Double fault handler.
///
/// A double fault usually indicates a serious kernel bug (e.g., stack overflow,
/// invalid IDT/GDT/TSS setup, or an exception while handling another exception).
/// Use serial output to avoid further faults if VGA isn't accessible.
extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    crate::serial_println!("EXCEPTION: DOUBLE FAULT");
    crate::serial_println!("Stack Frame: {:#?}", stack_frame);
    crate::serial_println!("Halting...");
    hlt_loop();
}

/// Smoke test: trigger a breakpoint exception.
///
/// This test uses `int3` to force the CPU to raise a breakpoint exception,
/// which should be handled by [`breakpoint_handler`].
#[test_case]
fn test_breakpoint_exception() {
    x86_64::instructions::interrupts::int3();
}

/// Test that the timer interrupt increments uptime (init already ran in test_kernel_main).
#[test_case]
fn test_uptime_increments() {
    // Ensure interrupts are enabled
    if !x86_64::instructions::interrupts::are_enabled() {
        x86_64::instructions::interrupts::enable();
    }
    
    let t0 = crate::uptime_ticks();
    // Wait for at least one timer interrupt to fire
    // Timer fires at ~18.2 Hz (default) or 100 Hz (if configured), so this should complete quickly
    let mut iterations = 0;
    const MAX_ITERATIONS: u32 = 10_000_000; // Safety limit to prevent infinite loop
    
    // Wait until uptime increases (timer interrupt fired)
    while crate::uptime_ticks() == t0 && iterations < MAX_ITERATIONS {
        x86_64::instructions::hlt(); // Halt and wait for interrupt
        iterations += 1;
    }
    
    let t1 = crate::uptime_ticks();
    assert!(t1 > t0, "uptime should increase ({} -> {}), iterations: {}", t0, t1, iterations);
    assert!(iterations < MAX_ITERATIONS, 
            "test timed out: uptime did not increase after {} iterations (interrupts may not be firing)", 
            iterations);
}
