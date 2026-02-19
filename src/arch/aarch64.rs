//! aarch64 (ARM 64-bit) stub.
//!
//! Provides the arch interface so the kernel compiles. Real bring-up would add:
//! - Boot: kernel loaded by QEMU/virt or U-Boot at a fixed address
//! - GIC for interrupts, generic timer
//! - MMU with ARM page tables
//! - PL011 or similar UART for serial
//! - Context switch (save/restore x19–x28, sp, lr, etc.)

use core::arch::asm;

pub fn init() {
    // Stub: no GIC, no timer, no UART init yet.
}

pub fn hlt_loop() -> ! {
    loop {
        // Wait for interrupt (WFI). Optional: enable IRQs first.
        unsafe { asm!("wfi") };
    }
}

pub fn exit_qemu(code: crate::QemuExitCode) {
    // QEMU virt machine: exit via semihosting or ARM semihosting.
    // For now we use the "runstate" approach: write to a known address
    // or use semihosting SYS_EXIT.
    let _ = code;
    // Stub: no isa-debug-exit on ARM; would use semihosting (e.g. 0x18/0x26).
}
