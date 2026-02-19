//! riscv64 (RISC-V 64-bit) stub.
//!
//! Provides the arch interface so the kernel compiles. Real bring-up would add:
//! - Boot: kernel loaded by QEMU/virt at 0x80200000
//! - PLIC/CLINT for interrupts and timer
//! - Sv39 page tables
//! - NS16550 UART (e.g. at 0x10000000 on virt)
//! - Context switch (save/restore s0–s11, sp, ra, etc.)

use core::arch::asm;

pub fn init() {
    // Stub: no PLIC/CLINT, no UART init yet.
}

pub fn hlt_loop() -> ! {
    loop {
        // Wait for interrupt.
        unsafe { asm!("wfi") };
    }
}

pub fn exit_qemu(code: crate::QemuExitCode) {
    // QEMU virt: exit via the "test device" at 0x100000 (see riscv virt machine).
    // Write (value << 1) | 1 to 0x100000 to exit with code.
    let _ = code;
    // Stub: exact address/semantics depend on QEMU riscv virt.
}
