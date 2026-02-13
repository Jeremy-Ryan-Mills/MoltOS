//! Interrupt and exception setup.
//!
//! This module builds and loads the CPU’s IDT (Interrupt Descriptor Table),
//! sets up handlers for a few exceptions, and wires up PIC-based hardware IRQs
//! (timer + keyboard). It also provides a small enum for mapping IRQ lines to
//! IDT vector indices.

use lazy_static::lazy_static;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use pic8259::ChainedPics;
use spin;

use crate::gdt;
use crate::println;
use crate::print;
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

/// Load the IDT into the CPU.
///
/// Call this during early boot after the GDT/TSS is set up.
pub fn init_idt() {
    IDT.load();
}

/// Timer IRQ handler (PIT, IRQ0).
///
/// Prints a dot so you can visually confirm interrupts are firing, then sends
/// an EOI (end-of-interrupt) to the PIC so it can deliver further IRQs.
extern "x86-interrupt" fn timer_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    print!(".");

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Timer.as_u8());
    }
}

/// Keyboard IRQ handler (PS/2, IRQ1).
///
/// Reads a scancode from port `0x60`, feeds it into the `pc_keyboard` decoder,
/// and prints either the decoded Unicode character or the raw key value.
/// Finally, sends an EOI to the PIC.
extern "x86-interrupt" fn keyboard_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    use x86_64::instructions::port::Port;

    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };
    crate::task::keyboard::add_scancode(scancode);

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8());
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

    println!("EXCEPTION: PAGE FAULT");
    println!("Accessed Address: {:?}", Cr2::read());
    println!("Error Code: {:?}", error_code);
    println!("{:#?}", stack_frame);
    hlt_loop();
}

/// Double fault handler.
///
/// A double fault usually indicates a serious kernel bug (e.g., stack overflow,
/// invalid IDT/GDT/TSS setup, or an exception while handling another exception).
/// We panic here so you get a message instead of silently resetting.
extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

/// Smoke test: trigger a breakpoint exception.
///
/// This test uses `int3` to force the CPU to raise a breakpoint exception,
/// which should be handled by [`breakpoint_handler`].
#[test_case]
fn test_breakpoint_exception() {
    x86_64::instructions::interrupts::int3();
}
