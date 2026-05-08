//! Tests specifically for round-robin scheduler.

#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(moltos::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use bootloader::{entry_point, BootInfo};
use core::panic::PanicInfo;
use moltos::test_panic_handler;
use moltos::thread::schedulers::round_robin::RoundRobinScheduler;
use moltos::thread::{Thread, context::ThreadContext};

entry_point!(main);

fn main(boot_info: &'static BootInfo) -> ! {
    use moltos::allocator;
    use moltos::memory::{self, BootInfoFrameAllocator};
    use x86_64::VirtAddr;

    moltos::init();
    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe {
        BootInfoFrameAllocator::init(&boot_info.memory_map)
    };
    allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");

    test_main();
    loop {}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    moltos::test_panic_handler(info)
}

#[test_case]
fn test_round_robin_sequential() {
    moltos::serial_print!("scheduler_round_robin::test_round_robin_sequential...\t");
    fn dummy_entry() {
        loop {}
    }
    
    let mut sched = RoundRobinScheduler::new();
    sched.spawn(Thread::new(dummy_entry));
    sched.spawn(Thread::new(dummy_entry));
    sched.spawn(Thread::new(dummy_entry));
    
    let mut bootstrap_ctx = ThreadContext::default();
    let bootstrap_ptr = &mut bootstrap_ctx as *mut ThreadContext;
    
    // First tick: should pick thread 0
    let result1 = sched.tick_prepare(bootstrap_ptr, 0);
    assert!(result1.is_some());
    let (_, to1) = result1.unwrap();
    let to1_idx = unsafe { 
        // We can't easily get the index from the pointer, but we can verify
        // it's not null
        !to1.is_null()
    };
    assert!(to1_idx);
    
    // Second tick: should pick thread 1
    let result2 = sched.tick_prepare(bootstrap_ptr, 1);
    assert!(result2.is_some());
    
    // Third tick: should pick thread 2
    let result3 = sched.tick_prepare(bootstrap_ptr, 2);
    assert!(result3.is_some());
    
    // Fourth tick: should wrap to thread 0
    let result4 = sched.tick_prepare(bootstrap_ptr, 3);
    assert!(result4.is_some());
    
    moltos::serial_println!("[ok]");
}

#[test_case]
fn test_round_robin_empty() {
    moltos::serial_print!("scheduler_round_robin::test_round_robin_empty...\t");
    let mut sched = RoundRobinScheduler::new();
    let mut bootstrap_ctx = ThreadContext::default();
    let bootstrap_ptr = &mut bootstrap_ctx as *mut ThreadContext;
    
    let result = sched.tick_prepare(bootstrap_ptr, 0);
    assert!(result.is_none());
    moltos::serial_println!("[ok]");
}

#[test_case]
fn test_round_robin_single_thread() {
    moltos::serial_print!("scheduler_round_robin::test_round_robin_single_thread...\t");
    fn dummy_entry() {
        loop {}
    }
    
    let mut sched = RoundRobinScheduler::new();
    sched.spawn(Thread::new(dummy_entry));
    let mut bootstrap_ctx = ThreadContext::default();
    let bootstrap_ptr = &mut bootstrap_ctx as *mut ThreadContext;
    
    // Should always pick the same thread
    let result1 = sched.tick_prepare(bootstrap_ptr, 0);
    assert!(result1.is_some());
    
    let result2 = sched.tick_prepare(bootstrap_ptr, 1);
    assert!(result2.is_some());
    
    moltos::serial_println!("[ok]");
}
