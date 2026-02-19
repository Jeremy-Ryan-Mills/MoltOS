#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(chronos::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;
use core::panic::PanicInfo;

// --- x86_64: full kernel with bootloader, heap, threads, shell ---
#[cfg(target_arch = "x86_64")]
use bootloader::{BootInfo, entry_point};
#[cfg(target_arch = "x86_64")]
use chronos::task::{Task, executor::Executor, shell};
#[cfg(target_arch = "x86_64")]
use chronos::thread::{self, Thread};
#[cfg(target_arch = "x86_64")]
use x86_64::VirtAddr;

#[cfg(target_arch = "x86_64")]
entry_point!(kernel_main);

#[cfg(target_arch = "x86_64")]
fn idle_entry() {
    loop {
        x86_64::instructions::hlt();
    }
}

#[cfg(target_arch = "x86_64")]
fn executor_entry() {
    let mut executor = Executor::new();
    executor.spawn(Task::new(shell::run_shell()));
    executor.spawn(Task::new(heartbeat()));
    executor.run();
}

#[cfg(target_arch = "x86_64")]
async fn heartbeat() {
    use chronos::task::sleep::Sleep;
    loop {
        Sleep::new(200).await;
        chronos::println!("[heartbeat]");
    }
}

#[cfg(target_arch = "x86_64")]
fn kernel_main(boot_info: &'static BootInfo) -> ! {
    chronos::init();

    use chronos::allocator;
    use chronos::memory::{self, BootInfoFrameAllocator};

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe {
        BootInfoFrameAllocator::init(&boot_info.memory_map)
    };

    allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");

    memory::init_memory_map(&boot_info.memory_map);

    thread::SCHEDULER.lock().spawn(Thread::new(idle_entry));
    thread::SCHEDULER.lock().spawn(Thread::new(executor_entry));

    chronos::println!("Chronos: threads + shell. Timer preempts round-robin.");
    thread::enter_scheduler();
}

// --- ARM (aarch64) and RISC-V (riscv64): minimal entry (no bootloader, no heap/threads yet) ---
#[cfg(any(target_arch = "aarch64", target_arch = "riscv64"))]
#[no_mangle]
pub unsafe fn _start() -> ! {
    chronos::init();
    chronos::println!("Chronos (stub) on this arch. Halting.");
    chronos::hlt_loop();
}

// --- Panic handlers (all arches) ---
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    chronos::println!("{}", info);
    chronos::hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    chronos::test_panic_handler(info);
}
