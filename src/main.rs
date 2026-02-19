#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(chronos::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;
use alloc::{boxed::Box, vec, vec::Vec, rc::Rc};

use bootloader::{BootInfo, entry_point};
use chronos::println;
use chronos::task::{Task, executor::Executor, shell};
use x86_64::VirtAddr;
use core::panic::PanicInfo;

entry_point!(kernel_main);

/// Demo task: prints every 200 timer ticks to show async Sleep working.
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

    let mut executor = Executor::new();
    executor.spawn(Task::new(shell::run_shell()));
    // executor.spawn(Task::new(heartbeat()));
    executor.run();


    // For testing
    #[cfg(test)]
    test_main();

    println!("It didnt crash yay");
    chronos::hlt_loop();
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
