# Chronos OS in Rust

This is a personal project to develop a Kernel in Rust to get more comfortable with Rust. 

To build the rust project, run:
```
cargo build
```

To run the rust project with QEMU, run:
```
// For x86_64
cargo run

// For ARM (In progress)
cargo build -Z build-std --target aarch64-unknown-none

// For RISC-V (In progress)
cargo build -Z build-std --target riscv64gc-unknown-none-elf
```