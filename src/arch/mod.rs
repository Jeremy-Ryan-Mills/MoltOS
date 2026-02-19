//! Architecture-specific code.
//!
//! Each supported target has a submodule that provides:
//! - `init()` — set up CPU, interrupts, and basic I/O
//! - `hlt_loop()` — idle the CPU until the next interrupt
//! - `exit_qemu(code)` — exit QEMU with a status (for tests)
//! - Serial/console output used by the rest of the kernel

#[cfg(target_arch = "x86_64")]
pub mod x86_64;

#[cfg(target_arch = "aarch64")]
pub mod aarch64;

#[cfg(target_arch = "riscv64")]
pub mod riscv64;
