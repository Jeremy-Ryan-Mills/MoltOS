# Chronos OS

A hobby operating system kernel written in Rust, designed to run on x86_64 architecture. Chronos demonstrates core OS concepts including memory management, multitasking, interrupt handling, and async/await support.

## Features

### Core Kernel Features

- **Memory Management**
  - Virtual memory with paging (4KB pages)
  - Heap allocation with multiple allocator implementations (bump, linked list, fixed-size block)
  - Physical frame allocation from bootloader memory map
  - VGA buffer identity mapping

- **Interrupt Handling**
  - Interrupt Descriptor Table (IDT) setup
  - Programmable Interval Timer (PIT) for preemptive scheduling
  - Keyboard interrupt handling (PS/2)
  - Exception handling (page faults, double faults, breakpoints)

- **Multitasking**
  - Preemptive kernel threads with separate stacks
  - EEVDF (Earliest Eligible Virtual Deadline First) scheduler for fair CPU time allocation
  - Round-robin scheduler (alternative implementation)
  - Context switching with callee-saved register preservation

- **Async/Await Support**
  - Custom async executor for cooperative multitasking
  - Future-based task scheduling
  - Timer-based async sleep
  - Keyboard input stream as async future

- **User Interface**
  - VGA text mode output (80x25)
  - Serial port output for debugging
  - Interactive shell with command parsing
  - Built-in commands: `help`, `clear`, `echo`, `uptime`, `mem`

## Architecture

### Boot Process

1. Bootloader loads kernel and provides memory map
2. Kernel initializes GDT/TSS for segmentation
3. Sets up IDT for interrupt handling
4. Configures PIC (Programmable Interrupt Controller)
5. Initializes heap allocator
6. Maps VGA buffer
7. Spawns executor thread
8. Enters scheduler loop

### Thread Model

- **Executor Thread**: Runs async executor that polls futures (shell, heartbeat)
- Each thread has its own 16KB kernel stack
- Threads are preempted by timer interrupts (~18.2 Hz default)
- Context switching saves/restores CPU state (registers, stack pointer, instruction pointer)

### Memory Layout

- Kernel code/data: loaded by bootloader
- Heap: 100 KiB at virtual address `0x4444_4444_0000`
- Stacks: 16 KiB per thread, allocated from heap
- VGA buffer: identity-mapped at `0xb8000`

## Building

### Prerequisites

- Rust toolchain (nightly recommended)
- `bootimage` crate for building bootable images
- QEMU for running the kernel

### Build Commands

```bash
# Build the kernel
cargo build

# Build and run in QEMU
cargo run

# Run tests
cargo test
```

### QEMU Configuration

The kernel is configured to run in QEMU with:
- Serial output to stdio
- Debug exit device for test framework
- VGA display

## Project Structure

```
src/
├── main.rs              # Kernel entry point, thread spawning
├── lib.rs               # Core kernel library, initialization
├── gdt.rs               # Global Descriptor Table setup
├── interrupts.rs        # IDT, PIC, interrupt handlers
├── memory.rs            # Page table management, frame allocation
├── allocator.rs         # Heap allocator implementations
├── vga_buffer.rs        # VGA text mode driver
├── serial.rs            # Serial port output
├── task/                # Async task system
│   ├── executor.rs      # Async executor implementation
│   ├── shell.rs         # Interactive shell
│   ├── keyboard.rs      # Keyboard input stream
│   └── sleep.rs         # Timer-based async sleep
└── thread/              # Threading system
    ├── scheduler.rs     # Scheduler wrapper
    ├── schedulers/       # Scheduler implementations
    │   ├── eevdf.rs     # EEVDF scheduler
    │   └── round_robin.rs
    ├── context.rs       # Context switching assembly
    ├── stack.rs         # Thread stack management
    └── thread.rs        # Thread data structures
```

## Testing

The kernel includes a custom test framework that runs tests in QEMU:

- `test_breakpoint_exception`: Tests exception handling
- `test_uptime_increments`: Verifies timer interrupt functionality
- `threading`: Tests thread creation and scheduling
- `scheduler_eevdf`: Tests EEVDF scheduler behavior
- `scheduler_round_robin`: Tests round-robin scheduler

Run tests with:
```bash
cargo test
```

## Shell Commands

Once the kernel boots, you'll see an interactive shell prompt. Available commands:

- `help` - Show available commands
- `clear` / `cls` - Clear the screen
- `echo <text>` - Print text
- `uptime` - Show system uptime in timer ticks
- `mem` / `memory` - Dump memory map

## Technical Details

### Scheduler

The default scheduler is EEVDF (Earliest Eligible Virtual Deadline First), which provides:
- Fair CPU time allocation based on thread weights
- Virtual runtime tracking for each thread
- Lag tolerance to prevent thread starvation
- Configurable thread priorities via weights

### Async Executor

The async executor uses:
- `BTreeMap` for task storage
- `ArrayQueue` for ready task queue
- Waker-based task wakeup
- Cooperative polling of futures

### Memory Safety

- Uses Rust's type system for memory safety
- `unsafe` blocks are isolated and documented
- No use of `unsafe` in high-level code paths
- Stack overflow protection via separate kernel stacks
