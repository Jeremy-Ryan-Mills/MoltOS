#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(chronos::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;
use alloc::{boxed::Box, vec, vec::Vec, rc::Rc};

use bootloader::{BootInfo, entry_point};
use chronos::println;
use chronos::task::{Task, keyboard, executor::Executor};
use x86_64::VirtAddr;
use core::panic::PanicInfo;

entry_point!(kernel_main);

async fn async_number() -> u32 {
    42
}

async fn example_task() {
    let number = async_number().await;
    println!("number: {}", number);
}

fn kernel_main(boot_info: &'static BootInfo) -> ! {

    println!("Hello World{}", "!");
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


    let mut executor = Executor::new();
    executor.spawn(Task::new(example_task()));
    executor.spawn(Task::new(keyboard::print_keypresses())); // new
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
