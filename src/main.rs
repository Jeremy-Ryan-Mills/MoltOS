#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(chronos::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use bootloader::{BootInfo, entry_point};
use chronos::println;
use chronos::task::{keyboard, Task, executor::Executor, shell};
use chronos::thread::{self, Thread};
use x86_64::VirtAddr;
use core::panic::PanicInfo;
use spin::Mutex;

entry_point!(kernel_main);

/// Executor is created in kernel_main (before any context switch) to avoid
/// allocation in the executor thread, which may have a smaller or different
/// stack/context.
static EXECUTOR: Mutex<Option<Executor>> = Mutex::new(None);

/// Idle thread: yields immediately so executor thread can run.
/// This thread exists only to give the scheduler something to switch to,
/// but it immediately yields back so the executor gets all CPU time.
fn idle_entry() {
    loop {
        // Yield immediately - just enable interrupts and let timer switch us away
        x86_64::instructions::interrupts::enable();
        x86_64::instructions::hlt();
    }
}

/// Runs in a dedicated thread: takes the pre-created executor and runs it.
fn executor_entry() {
    chronos::serial_println!("[executor] Thread entry point reached");
    println!("[executor] Thread started");
    chronos::serial_println!("[executor] About to lock EXECUTOR");
    let mut executor = EXECUTOR.lock().take().expect("executor not initialized");
    chronos::serial_println!("[executor] Got executor from static");
    println!("[executor] Entering run loop");
    chronos::serial_println!("[executor] Calling executor.run()");
    executor.run();
}

async fn heartbeat() {
    use chronos::task::sleep::Sleep;
    loop {
        Sleep::new(200).await;
        //chronos::println!("[heartbeat]");
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

    // Identity-map VGA at 0xb8000 so the VGA driver's hardcoded address is valid.
    memory::map_vga_buffer(&mut mapper, &mut frame_allocator);

    memory::init_memory_map(&boot_info.memory_map);

    // Init keyboard scancode queue so IRQ handler can enqueue before the shell runs.
    keyboard::init_scancode_queue();

    // Create executor and spawn tasks in bootstrap context (before any context switch).
    // This avoids allocation in the executor thread.
    chronos::serial_println!("[kernel_main] Creating executor");
    let mut executor = Executor::new();
    chronos::serial_println!("[kernel_main] Spawning shell task");
    executor.spawn(Task::new(shell::run_shell()));
    chronos::serial_println!("[kernel_main] Spawning heartbeat task");
    executor.spawn(Task::new(heartbeat()));
    chronos::serial_println!("[kernel_main] Storing executor in static");
    *EXECUTOR.lock() = Some(executor);
    chronos::serial_println!("[kernel_main] Executor stored successfully");

    // Spawn executor thread - this is the only thread we need
    // The executor handles sleeping when idle, so we don't need a separate idle thread
    chronos::serial_println!("[kernel_main] Spawning executor thread");
    thread::SCHEDULER.lock().spawn(Thread::new(executor_entry));

    println!("Chronos: threads + shell. Timer preempts round-robin.");
    println!("Scheduler has {} threads", thread::SCHEDULER.lock().len());
    chronos::serial_println!("[kernel_main] About to enter scheduler");
    thread::enter_scheduler();
}

/// This function is called on panic.
/// Uses serial first so we see the panic even if VGA is broken.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    chronos::serial_println!("PANIC: {}", info);
    println!("{}", info);
    chronos::hlt_loop();
}

/// This function is called on panic while testing.
#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    chronos::test_panic_handler(info)
}
