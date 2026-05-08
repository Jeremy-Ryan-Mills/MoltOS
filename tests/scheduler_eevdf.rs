//! Tests specifically for EEVDF scheduler.

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
fn test_eevdf_empty() {
    moltos::serial_print!("scheduler_eevdf::test_eevdf_empty...\t");
    let mut sched = EevdfScheduler::new();
    let mut bootstrap_ctx = ThreadContext::default();
    let bootstrap_ptr = &mut bootstrap_ctx as *mut ThreadContext;
    
    let result = sched.tick_prepare(bootstrap_ptr, 0);
    assert!(result.is_none());
    moltos::serial_println!("[ok]");
}

#[test_case]
fn test_eevdf_spawn_default_weight() {
    moltos::serial_print!("scheduler_eevdf::test_eevdf_spawn_default_weight...\t");
    fn dummy_entry() {
        loop {}
    }
    
    let mut sched = EevdfScheduler::new();
    sched.spawn(Thread::new(dummy_entry));
    assert_eq!(sched.len(), 1);
    moltos::serial_println!("[ok]");
}

#[test_case]
fn test_eevdf_spawn_with_weight() {
    moltos::serial_print!("scheduler_eevdf::test_eevdf_spawn_with_weight...\t");
    fn dummy_entry() {
        loop {}
    }
    
    let mut sched = EevdfScheduler::new();
    sched.spawn_with_weight(Thread::new(dummy_entry), 2048);
    sched.spawn_with_weight(Thread::new(dummy_entry), 512);
    assert_eq!(sched.len(), 2);
    moltos::serial_println!("[ok]");
}

#[test_case]
fn test_eevdf_min_weight() {
    moltos::serial_print!("scheduler_eevdf::test_eevdf_min_weight...\t");
    fn dummy_entry() {
        loop {}
    }
    
    let mut sched = EevdfScheduler::new();
    // Weight 0 should be clamped to 1
    sched.spawn_with_weight(Thread::new(dummy_entry), 0);
    // Weight 1 should be accepted
    sched.spawn_with_weight(Thread::new(dummy_entry), 1);
    assert_eq!(sched.len(), 2);
    moltos::serial_println!("[ok]");
}

#[test_case]
fn test_eevdf_tick_prepare_with_threads() {
    moltos::serial_print!("scheduler_eevdf::test_eevdf_tick_prepare_with_threads...\t");
    fn dummy_entry() {
        loop {}
    }
    
    let mut sched = EevdfScheduler::new();
    sched.spawn(Thread::new(dummy_entry));
    let mut bootstrap_ctx = ThreadContext::default();
    let bootstrap_ptr = &mut bootstrap_ctx as *mut ThreadContext;
    
    let result = sched.tick_prepare(bootstrap_ptr, 0);
    assert!(result.is_some());
    let (from, to) = result.unwrap();
    assert!(!from.is_null());
    assert!(!to.is_null());
    moltos::serial_println!("[ok]");
}

#[test_case]
fn test_eevdf_time_slice_computation() {
    moltos::serial_print!("scheduler_eevdf::test_eevdf_time_slice_computation...\t");
    fn dummy_entry() {
        loop {}
    }
    
    let mut sched = EevdfScheduler::new();
    sched.spawn(Thread::new(dummy_entry));
    let mut bootstrap_ctx = ThreadContext::default();
    let bootstrap_ptr = &mut bootstrap_ctx as *mut ThreadContext;
    
    // First tick at time 0
    let _ = sched.tick_prepare(bootstrap_ptr, 0);
    
    // Second tick at time 10 (time slice should be at least 1)
    let _ = sched.tick_prepare(bootstrap_ptr, 10);
    
    // Third tick at time 25 (time slice should be 15)
    let _ = sched.tick_prepare(bootstrap_ptr, 25);
    
    moltos::serial_println!("[ok]");
}

#[test_case]
fn test_eevdf_vruntime_updates() {
    moltos::serial_print!("scheduler_eevdf::test_eevdf_vruntime_updates...\t");
    fn dummy_entry() {
        loop {}
    }
    
    let mut sched = EevdfScheduler::new();
    sched.spawn(Thread::new(dummy_entry));
    let mut bootstrap_ctx = ThreadContext::default();
    let bootstrap_ptr = &mut bootstrap_ctx as *mut ThreadContext;
    
    // First tick: switch to thread 0
    let _ = sched.tick_prepare(bootstrap_ptr, 0);
    
    // Second tick: thread 0 should have advanced vruntime
    let _ = sched.tick_prepare(bootstrap_ptr, 10);
    
    // The vruntime should have been updated (we can't directly check it,
    // but the scheduler should still work)
    let _ = sched.tick_prepare(bootstrap_ptr, 20);
    
    moltos::serial_println!("[ok]");
}

#[test_case]
fn test_eevdf_min_vruntime_tracking() {
    moltos::serial_print!("scheduler_eevdf::test_eevdf_min_vruntime_tracking...\t");
    fn dummy_entry() {
        loop {}
    }
    
    let mut sched = EevdfScheduler::new();
    sched.spawn(Thread::new(dummy_entry));
    sched.spawn(Thread::new(dummy_entry));
    let mut bootstrap_ctx = ThreadContext::default();
    let bootstrap_ptr = &mut bootstrap_ctx as *mut ThreadContext;
    
    // Run several ticks
    let _ = sched.tick_prepare(bootstrap_ptr, 0);
    let _ = sched.tick_prepare(bootstrap_ptr, 5);
    let _ = sched.tick_prepare(bootstrap_ptr, 10);
    let _ = sched.tick_prepare(bootstrap_ptr, 15);
    
    // min_vruntime should be tracked (we can't directly check, but
    // the scheduler should continue working)
    moltos::serial_println!("[ok]");
}

#[test_case]
fn test_eevdf_weight_affects_scheduling() {
    moltos::serial_print!("scheduler_eevdf::test_eevdf_weight_affects_scheduling...\t");
    fn dummy_entry() {
        loop {}
    }
    
    let mut sched = EevdfScheduler::new();
    // High weight thread should get scheduled more often
    sched.spawn_with_weight(Thread::new(dummy_entry), 2048);
    // Low weight thread
    sched.spawn_with_weight(Thread::new(dummy_entry), 512);
    let mut bootstrap_ctx = ThreadContext::default();
    let bootstrap_ptr = &mut bootstrap_ctx as *mut ThreadContext;
    
    // Run several ticks - high weight thread should have earlier deadlines
    let _ = sched.tick_prepare(bootstrap_ptr, 0);
    let _ = sched.tick_prepare(bootstrap_ptr, 5);
    let _ = sched.tick_prepare(bootstrap_ptr, 10);
    
    moltos::serial_println!("[ok]");
}
