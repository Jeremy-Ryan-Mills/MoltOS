//! Kernel crate root.
//!
//! This is the top-level entry point for the kernel as a Rust library crate.
//! It wires up core subsystems (GDT/TSS, IDT/PIC, basic output), provides a
//! simple custom test framework for `cargo test` in QEMU, and includes a few
//! small utilities used across the kernel.

#![no_std]
#![cfg_attr(test, no_main)]
#![feature(custom_test_frameworks)]
#![feature(abi_x86_interrupt)]
#![test_runner(crate::test_runner)]
#![reexport_test_harness_main = "test_main"]

#[cfg(test)]
use bootloader::{entry_point, BootInfo};

extern crate alloc;
use core::panic::PanicInfo;

pub mod gdt;
pub mod interrupts;
pub mod serial;
pub mod vga_buffer;
pub mod memory;
pub mod allocator;
pub mod task;

#[cfg(test)]
entry_point!(test_kernel_main);

/// Trait implemented by things that can be run as tests.
///
/// We use this to print a test name before running it and mark `[ok]` on
/// success. The test harness passes us a slice of `&dyn Testable`.
pub trait Testable {
    fn run(&self) -> ();
}

/// Blanket impl so plain `fn()` tests can be used directly.
///
/// Any zero-arg function can be treated as a test.
impl<T> Testable for T
where
    T: Fn(),
{
    fn run(&self) {
        serial_print!("{}...\t", core::any::type_name::<T>());
        self();
        serial_println!("[ok]");
    }
}

/// Exit codes understood by QEMU when using the `isa-debug-exit` device.
///
/// Writing these values to port `0xF4` allows tests to signal success/failure
/// to the host without needing a full userspace or filesystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QemuExitCode {
    /// Test run completed successfully.
    Success = 0x10,
    /// At least one test failed (or a panic occurred).
    Failed = 0x11,
}

/// Exit QEMU with a specific status code.
///
/// This relies on QEMU being launched with the debug exit device enabled
/// (commonly `-device isa-debug-exit,iobase=0xf4,iosize=0x04`).
pub fn exit_qemu(exit_code: QemuExitCode) {
    use x86_64::instructions::port::Port;

    unsafe {
        let mut port = Port::new(0xf4);
        port.write(exit_code as u32);
    }
}

/// Initialize core CPU/kernel state needed for interrupts and basic runtime.
///
/// Order matters here:
/// - Load GDT/TSS (needed for IST stacks like double fault)
/// - Load IDT
/// - Initialize the PICs (enable delivery of IRQs)
/// - Enable CPU interrupts
pub fn init() {
    gdt::init();
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();
}


/// Custom test runner used by the `custom_test_frameworks` feature.
///
/// Prints test count, executes tests, then exits QEMU with a success code.
pub fn test_runner(tests: &[&dyn Testable]) {
    serial_println!("Running {} tests", tests.len());
    for test in tests {
        test.run();
    }
    exit_qemu(QemuExitCode::Success);
}

/// Panic handler used during `cargo test`.
///
/// Prints the panic information over serial, exits QEMU with a failure code,
/// and then halts the CPU.
pub fn test_panic_handler(info: &PanicInfo) -> ! {
    serial_println!("[failed]\n");
    serial_println!("Error: {}\n", info);
    exit_qemu(QemuExitCode::Failed);
    hlt_loop();
}

#[cfg(test)]
fn test_kernel_main(_boot_info: &'static BootInfo) -> ! {
    // like before
    init();
    test_main();
    hlt_loop();
}

/// Panic handler for test builds.
///
/// Delegates to [`test_panic_handler`].
#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    test_panic_handler(info)
}

/// Halt-loop used when there's nothing else to do.
///
/// `hlt` sleeps the CPU until the next interrupt, which is nicer than spinning
/// at 100% in QEMU.
pub fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}
