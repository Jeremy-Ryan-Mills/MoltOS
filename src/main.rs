#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(chronos::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use bootloader::{BootInfo, entry_point};
use chronos::println;
use chronos::task::{Task, executor::Executor, shell};
use chronos::thread::{self, Thread};
use x86_64::VirtAddr;
use core::panic::PanicInfo;

entry_point!(kernel_main);

/// Idle thread: just halts until the next interrupt (timer will switch away).
fn idle_entry() {
    loop {
        x86_64::instructions::hlt();
    }
}

/// Runs in a dedicated thread: the async executor (shell + heartbeat).
fn executor_entry() {
    let mut executor = Executor::new();
    executor.spawn(Task::new(shell::run_shell()));
    executor.spawn(Task::new(heartbeat()));
    executor.run();
}

async fn heartbeat() {
    use chronos::task::sleep::Sleep;
    loop {
        Sleep::new(200).await;
        chronos::println!("[heartbeat]");
    }
}

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

    // Spawn kernel threads: idle and executor (shell + async tasks).
    thread::SCHEDULER.lock().spawn(Thread::new(idle_entry));
    thread::SCHEDULER.lock().spawn(Thread::new(executor_entry));

    println!("Chronos: threads + shell. Timer preempts round-robin.");
    thread::enter_scheduler();
}

/// This function is called on panic.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    chronos::hlt_loop();
}

/// This function is called on panic while testing.
#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    chronos::test_panic_handler(info)
}
