//! Kernel crate root.
//!
//! This is the top-level entry point for the kernel as a Rust library crate.
//! It wires up core subsystems (GDT/TSS, IDT/PIC, basic output on x86_64),
//! provides a simple custom test framework for `cargo test` in QEMU, and
//! includes architecture-agnostic utilities. ARM (aarch64) and RISC-V (riscv64)
//! are supported with stub implementations that can be filled in per arch.

#![no_std]
#![cfg_attr(test, no_main)]
#![feature(custom_test_frameworks)]
#![cfg_attr(target_arch = "x86_64", feature(abi_x86_interrupt))]
#![test_runner(crate::test_runner)]
#![reexport_test_harness_main = "test_main"]

#[cfg(all(test, target_arch = "x86_64"))]
use bootloader::{entry_point, BootInfo};

extern crate alloc;
use core::panic::PanicInfo;

pub mod arch;

#[cfg(target_arch = "x86_64")]
pub mod gdt;
#[cfg(target_arch = "x86_64")]
pub mod interrupts;
#[cfg(target_arch = "x86_64")]
pub mod serial;
#[cfg(not(target_arch = "x86_64"))]
mod serial {
    /// No-op for non-x86 so serial_print! / serial_println! compile.
    #[doc(hidden)]
    pub fn _print(_: core::fmt::Arguments) {}
}

/// Console print for non-x86 (delegates to serial stub).
#[cfg(not(target_arch = "x86_64"))]
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => { $crate::serial::_print(core::fmt::format_args!($($arg)*)); };
}
/// Console println for non-x86.
#[cfg(not(target_arch = "x86_64"))]
#[macro_export]
macro_rules! println {
    () => { $crate::serial::_print(core::fmt::format_args!("\n")); };
    ($($arg:tt)*) => { $crate::serial::_print(core::fmt::format_args!("{}\n", format_args!($($arg)*))); };
}

#[cfg(target_arch = "x86_64")]
pub use interrupts::uptime_ticks;

#[cfg(target_arch = "x86_64")]
pub mod vga_buffer;
#[cfg(target_arch = "x86_64")]
pub mod memory;
#[cfg(target_arch = "x86_64")]
pub mod allocator;
#[cfg(target_arch = "x86_64")]
pub mod task;
#[cfg(target_arch = "x86_64")]
pub mod thread;

#[cfg(all(test, target_arch = "x86_64"))]
entry_point!(test_kernel_main);

/// Trait implemented by things that can be run as tests.
pub trait Testable {
    fn run(&self) -> ();
}

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

/// Exit codes understood by QEMU (x86: isa-debug-exit; other arches: arch-specific).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}

/// Exit QEMU with a specific status code (arch-specific implementation).
pub fn exit_qemu(exit_code: QemuExitCode) {
    #[cfg(target_arch = "x86_64")]
    arch::x86_64::exit_qemu(exit_code);
    #[cfg(target_arch = "aarch64")]
    arch::aarch64::exit_qemu(exit_code);
    #[cfg(target_arch = "riscv64")]
    arch::riscv64::exit_qemu(exit_code);
}

/// Initialize core CPU/kernel state (arch-specific).
pub fn init() {
    #[cfg(target_arch = "x86_64")]
    arch::x86_64::init();
    #[cfg(target_arch = "aarch64")]
    arch::aarch64::init();
    #[cfg(target_arch = "riscv64")]
    arch::riscv64::init();
}

/// Custom test runner: prints test count, runs tests, exits QEMU.
pub fn test_runner(tests: &[&dyn Testable]) {
    serial_println!("Running {} tests", tests.len());
    for test in tests {
        test.run();
    }
    exit_qemu(QemuExitCode::Success);
}

/// Panic handler used during `cargo test`.
pub fn test_panic_handler(info: &PanicInfo) -> ! {
    serial_println!("[failed]\n");
    serial_println!("Error: {}\n", info);
    exit_qemu(QemuExitCode::Failed);
    hlt_loop();
}

#[cfg(all(test, target_arch = "x86_64"))]
fn test_kernel_main(_boot_info: &'static BootInfo) -> ! {
    init();
    test_main();
    hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    #[cfg(target_arch = "x86_64")]
    {
        test_panic_handler(info);
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        serial_println!("[panic] {}", info);
        exit_qemu(QemuExitCode::Failed);
        hlt_loop();
    }
}

/// Halt-loop: idle the CPU until the next interrupt (arch-specific).
pub fn hlt_loop() -> ! {
    #[cfg(target_arch = "x86_64")]
    arch::x86_64::hlt_loop();
    #[cfg(target_arch = "aarch64")]
    arch::aarch64::hlt_loop();
    #[cfg(target_arch = "riscv64")]
    arch::riscv64::hlt_loop();
}
