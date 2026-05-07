/***
 * src/lib.rs
 *
 * Kernel crate root. Wires up GDT, IDT, PIC, and output subsystems.
 * Also provides the custom test framework used by cargo test in QEMU.
 */

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
pub use interrupts::uptime_ticks;
pub mod vga_buffer;
pub mod vga_mode13;
pub mod ata;
pub mod fs;
pub mod ffi;
pub mod memory;
pub mod allocator;
#[allow(path_statements)]
pub mod syscall;
pub mod task;
pub mod thread;
pub mod sync;

#[cfg(test)]
entry_point!(test_kernel_main);

pub trait Testable {
    fn run(&self);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QemuExitCode {
    Success = 0x10,
    Failed  = 0x11,
}

pub fn exit_qemu(exit_code: QemuExitCode) {
    use x86_64::instructions::port::Port;
    unsafe {
        let mut port = Port::new(0xf4);
        port.write(exit_code as u32);
    }
}

// Initialize GDT, IDT, PIC, and PIT. Order matters:
// GDT/TSS must be loaded before IDT entries that use IST stacks.
pub fn init() {
    gdt::init();
    interrupts::init_idt();
    unsafe {
        let mut pics = interrupts::PICS.lock();
        pics.initialize();
        interrupts::configure_pit(100);
        // Unmask IRQ0 (timer) and IRQ1 (keyboard).
        let [mut mask1, mask2] = pics.read_masks();
        mask1 &= !((1 << 0) | (1 << 1));
        pics.write_masks(mask1, mask2);
    }
    x86_64::instructions::interrupts::enable();
}

pub fn test_runner(tests: &[&dyn Testable]) {
    serial_println!("Running {} tests", tests.len());
    for test in tests {
        test.run();
    }
    exit_qemu(QemuExitCode::Success);
}

pub fn test_panic_handler(info: &PanicInfo) -> ! {
    serial_println!("[failed]\n");
    serial_println!("Error: {}\n", info);
    exit_qemu(QemuExitCode::Failed);
    hlt_loop();
}

#[cfg(test)]
fn test_kernel_main(boot_info: &'static BootInfo) -> ! {
    init();
    use allocator;
    use memory::{self, BootInfoFrameAllocator};
    use x86_64::VirtAddr;
    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };
    allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");
    test_main();
    hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    test_panic_handler(info)
}

pub fn hlt_loop() -> ! {
    loop { x86_64::instructions::hlt(); }
}
