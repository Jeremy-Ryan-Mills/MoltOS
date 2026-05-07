/***
 * src/main.rs
 *
 * Kernel entry point. Initializes memory, heap, scheduler, and spawns the
 * async executor thread before entering the scheduler.
 */

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
use chronos::ffi;
use x86_64::VirtAddr;
use core::panic::PanicInfo;
use spin::Mutex;

entry_point!(kernel_main);

// Executor is created before any context switch to avoid heap allocation
// from within the executor thread (which has a smaller stack).
static EXECUTOR: Mutex<Option<Executor>> = Mutex::new(None);

fn executor_entry() {
    let mut executor = EXECUTOR.lock().take().expect("executor not initialized");
    executor.run();
}

async fn heartbeat() {
    use chronos::task::sleep::Sleep;
    loop { Sleep::new(200).await; }
}

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    chronos::init();
    // init() enables interrupts. Disable them immediately so timer IRQs don't
    // consume the bootstrap stack during deep initialization (mapper.map_to,
    // heap init, etc). context_switch does sti before ret, so threads start
    // with interrupts enabled.
    x86_64::instructions::interrupts::disable();

    use chronos::allocator;
    use chronos::memory::{self, BootInfoFrameAllocator};

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };

    allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");

    memory::map_vga_buffer(&mut mapper, &mut frame_allocator);
    memory::init_memory_map(&boot_info.memory_map);
    chronos::vga_mode13::init(&mut mapper, &mut frame_allocator);

    match chronos::fs::init() {
        Ok(n)  => chronos::serial_println!("fs: loaded {} file(s)", n),
        Err(e) => chronos::serial_println!("fs: {:?} (no disk or empty archive)", e),
    }

    keyboard::init_scancode_queue();

    let mut executor = Executor::new();
    executor.spawn(Task::new(shell::run_shell()));
    executor.spawn(Task::new(heartbeat()));
    *EXECUTOR.lock() = Some(executor);

    thread::SCHEDULER.lock().spawn(
        Thread::with_stack_size(executor_entry, 256 * 1024) // 256 KiB for async futures
    );
    fn doom_entry() { ffi::doom_thread_entry(); }
    thread::SCHEDULER.lock().spawn(
        Thread::with_stack_size(doom_entry, 2 * 1024 * 1024) // 2 MiB — Doom needs it
    );

    println!("Chronos: threads + shell. Timer preempts round-robin.");
    println!("Scheduler has {} threads", thread::SCHEDULER.lock().len());
    thread::enter_scheduler();
}

#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    chronos::serial_println!("PANIC: {}", info);
    println!("{}", info);
    chronos::hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    chronos::test_panic_handler(info)
}
