//! Integration tests for the threading system.

#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(moltos::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use bootloader::{entry_point, BootInfo};
use core::panic::PanicInfo;
use moltos::test_panic_handler;
use moltos::thread::schedulers::eevdf::EevdfScheduler;
use moltos::thread::{Thread, SCHEDULER};
use moltos::thread::context::ThreadContext;

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
    test_panic_handler(info)
}

#[test_case]
fn test_scheduler_empty() {
    moltos::serial_print!("threading::test_scheduler_empty...\t");
    let mut sched = SCHEDULER.lock();
    assert_eq!(sched.len(), 0);
    let result = sched.tick_prepare(0);
    assert!(result.is_none());
    moltos::serial_println!("[ok]");
}

#[test_case]
fn test_scheduler_spawn() {
    moltos::serial_print!("threading::test_scheduler_spawn...\t");
    fn dummy_entry() {
        loop {}
    }
    
    let mut sched = EevdfScheduler::new();
    assert_eq!(sched.len(), 0);
    sched.spawn(Thread::new(dummy_entry));
    assert_eq!(sched.len(), 1);
    moltos::serial_println!("[ok]");
}

#[test_case]
fn test_scheduler_spawn_multiple() {
    moltos::serial_print!("threading::test_scheduler_spawn_multiple...\t");
    fn dummy_entry() {
        loop {}
    }
    
    let mut sched = EevdfScheduler::new();
    sched.spawn(Thread::new(dummy_entry));
    sched.spawn(Thread::new(dummy_entry));
    sched.spawn(Thread::new(dummy_entry));
    assert_eq!(sched.len(), 3);
    moltos::serial_println!("[ok]");
}

#[test_case]
fn test_scheduler_spawn_with_weight() {
    moltos::serial_print!("threading::test_scheduler_spawn_with_weight...\t");
    fn dummy_entry() {
        loop {}
    }
    
    let mut sched = EevdfScheduler::new();
    sched.spawn_with_weight(Thread::new(dummy_entry), 2048);
    assert_eq!(sched.len(), 1);
    moltos::serial_println!("[ok]");
}

#[test_case]
fn test_thread_ids_unique() {
    moltos::serial_print!("threading::test_thread_ids_unique...\t");
    fn dummy_entry() {
        loop {}
    }
    
    let thread1 = Thread::new(dummy_entry);
    let thread2 = Thread::new(dummy_entry);
    let thread3 = Thread::new(dummy_entry);
    
    assert_ne!(thread1.id, thread2.id);
    assert_ne!(thread2.id, thread3.id);
    assert_ne!(thread1.id, thread3.id);
    moltos::serial_println!("[ok]");
}

#[test_case]
fn test_scheduler_tick_prepare_returns_some_with_threads() {
    moltos::serial_print!("threading::test_scheduler_tick_prepare_returns_some_with_threads...\t");
    fn dummy_entry() {
        loop {}
    }
    
    let mut sched = EevdfScheduler::new();
    sched.spawn(Thread::new(dummy_entry));
    let mut bootstrap_ctx = ThreadContext::default();
    let result = sched.tick_prepare(&mut bootstrap_ctx as *mut ThreadContext, 0);
    assert!(result.is_some());
    let (from, to) = result.unwrap();
    assert!(!from.is_null());
    assert!(!to.is_null());
    moltos::serial_println!("[ok]");
}

#[test_case]
fn test_scheduler_multiple_ticks() {
    moltos::serial_print!("threading::test_scheduler_multiple_ticks...\t");
    fn dummy_entry() {
        loop {}
    }
    
    let mut sched = EevdfScheduler::new();
    sched.spawn(Thread::new(dummy_entry));
    sched.spawn(Thread::new(dummy_entry));
    let mut bootstrap_ctx = ThreadContext::default();
    let bootstrap_ptr = &mut bootstrap_ctx as *mut ThreadContext;
    
    let result1 = sched.tick_prepare(bootstrap_ptr, 0);
    assert!(result1.is_some());
    let result2 = sched.tick_prepare(bootstrap_ptr, 1);
    assert!(result2.is_some());
    let result3 = sched.tick_prepare(bootstrap_ptr, 2);
    assert!(result3.is_some());
    
    moltos::serial_println!("[ok]");
}

#[test_case]
fn test_scheduler_time_slice_tracking() {
    moltos::serial_print!("threading::test_scheduler_time_slice_tracking...\t");
    fn dummy_entry() {
        loop {}
    }
    
    let mut sched = EevdfScheduler::new();
    sched.spawn(Thread::new(dummy_entry));
    let mut bootstrap_ctx = ThreadContext::default();
    let bootstrap_ptr = &mut bootstrap_ctx as *mut ThreadContext;
    
    let _ = sched.tick_prepare(bootstrap_ptr, 0);
    let _ = sched.tick_prepare(bootstrap_ptr, 10);
    let _ = sched.tick_prepare(bootstrap_ptr, 25);
    
    moltos::serial_println!("[ok]");
}
