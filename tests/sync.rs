/***
 * tests/sync.rs
 *
 * Tests for scheduler block/unblock state and KMutex single-threaded behavior.
 * These run from bootstrap context (before enter_scheduler), so KMutex falls
 * back to spinning when current_thread_id() returns None. This is intentional —
 * the blocking code path is covered by kmutex_concurrent.rs.
 */

#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(moltos::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use bootloader::{entry_point, BootInfo};
use core::panic::PanicInfo;
use moltos::thread::schedulers::eevdf::EevdfScheduler;
use moltos::thread::{Thread, ThreadId, context::ThreadContext};
use moltos::sync::KMutex;

entry_point!(main);

fn main(boot_info: &'static BootInfo) -> ! {
    use moltos::allocator;
    use moltos::memory::{self, BootInfoFrameAllocator};
    use x86_64::VirtAddr;

    moltos::init();
    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };
    allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap init failed");

    test_main();
    loop {}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    moltos::test_panic_handler(info)
}

fn dummy() { loop {} }

// --- Scheduler block/unblock state tests ---

// Blocking the only thread makes tick_prepare return None (no runnable threads).
#[test_case]
fn test_block_removes_from_scheduling() {
    moltos::serial_print!("sync::test_block_removes_from_scheduling...\t");
    let mut sched = EevdfScheduler::new();
    sched.spawn(Thread::new(dummy));
    let mut bootstrap = ThreadContext::default();

    // First tick: switch to thread 0
    let r = sched.tick_prepare(&mut bootstrap, 0);
    assert!(r.is_some());

    // Block the current (only) thread
    let r = sched.block_current_and_prepare_switch(&mut bootstrap, 1);
    assert!(r.is_none(), "no runnable thread should remain after blocking the only one");

    // tick_prepare should also return None now
    let r = sched.tick_prepare(&mut bootstrap, 2);
    assert!(r.is_none());
    moltos::serial_println!("[ok]");
}

// After unblocking, tick_prepare returns Some again.
#[test_case]
fn test_unblock_restores_runnability() {
    moltos::serial_print!("sync::test_unblock_restores_runnability...\t");
    let mut sched = EevdfScheduler::new();
    sched.spawn(Thread::new(dummy));
    let mut bootstrap = ThreadContext::default();

    let _ = sched.tick_prepare(&mut bootstrap, 0);
    let id = sched.current_thread_id().expect("should have a current thread");

    let _ = sched.block_current_and_prepare_switch(&mut bootstrap, 1);
    assert!(sched.tick_prepare(&mut bootstrap, 2).is_none());

    sched.unblock_thread(id);
    assert!(sched.tick_prepare(&mut bootstrap, 3).is_some(), "thread should be schedulable after unblock");
    moltos::serial_println!("[ok]");
}

// With two threads, blocking one still lets the other run.
#[test_case]
fn test_blocked_thread_is_skipped() {
    moltos::serial_print!("sync::test_blocked_thread_is_skipped...\t");
    let mut sched = EevdfScheduler::new();
    sched.spawn(Thread::new(dummy));
    sched.spawn(Thread::new(dummy));
    let mut bootstrap = ThreadContext::default();

    // Switch to whichever thread EEVDF picks first
    let _ = sched.tick_prepare(&mut bootstrap, 0);
    let first_id = sched.current_thread_id().unwrap();

    // Block that thread
    let r = sched.block_current_and_prepare_switch(&mut bootstrap, 1);
    assert!(r.is_some(), "should switch to the other thread");

    // The new current thread should not be the blocked one
    let second_id = sched.current_thread_id().unwrap();
    assert_ne!(first_id, second_id, "scheduler should have switched to the non-blocked thread");

    // Further ticks should keep running the unblocked thread
    let r = sched.tick_prepare(&mut bootstrap, 10);
    assert!(r.is_some());
    moltos::serial_println!("[ok]");
}

// Unblocking a thread that never ran (was never switched to) is a no-op.
#[test_case]
fn test_unblock_unknown_id_is_noop() {
    moltos::serial_print!("sync::test_unblock_unknown_id_is_noop...\t");
    let mut sched = EevdfScheduler::new();
    sched.spawn(Thread::new(dummy));
    let mut bootstrap = ThreadContext::default();

    let _ = sched.tick_prepare(&mut bootstrap, 0);

    // Unblock a made-up ID — should not panic or corrupt state
    sched.unblock_thread(ThreadId(999999));

    // Scheduler should still work
    let r = sched.tick_prepare(&mut bootstrap, 1);
    assert!(r.is_some());
    moltos::serial_println!("[ok]");
}

// Blocking all threads returns None from both block_ and tick_prepare.
#[test_case]
fn test_all_threads_blocked() {
    moltos::serial_print!("sync::test_all_threads_blocked...\t");
    let mut sched = EevdfScheduler::new();
    sched.spawn(Thread::new(dummy));
    sched.spawn(Thread::new(dummy));
    let mut bootstrap = ThreadContext::default();

    // Switch to first thread, block it → switches to second
    let _ = sched.tick_prepare(&mut bootstrap, 0);
    let r = sched.block_current_and_prepare_switch(&mut bootstrap, 1);
    assert!(r.is_some(), "should switch to second thread");

    // Block second thread too → nothing left
    let r = sched.block_current_and_prepare_switch(&mut bootstrap, 2);
    assert!(r.is_none(), "all threads blocked");
    moltos::serial_println!("[ok]");
}

// --- KMutex single-threaded tests ---
// These run from bootstrap so lock() spins (no thread to block). They verify
// the fast path and data access work correctly.

#[test_case]
fn test_kmutex_basic_lock_unlock() {
    moltos::serial_print!("sync::test_kmutex_basic_lock_unlock...\t");
    let m = KMutex::new(0u32);
    { *m.lock() = 42; }
    assert_eq!(*m.lock(), 42);
    moltos::serial_println!("[ok]");
}

#[test_case]
fn test_kmutex_guard_deref_mut() {
    moltos::serial_print!("sync::test_kmutex_guard_deref_mut...\t");
    let m = KMutex::new(alloc::vec![1u32, 2, 3]);
    {
        let mut g = m.lock();
        g.push(4);
    }
    assert_eq!(m.lock().len(), 4);
    moltos::serial_println!("[ok]");
}

#[test_case]
fn test_kmutex_sequential_locks() {
    moltos::serial_print!("sync::test_kmutex_sequential_locks...\t");
    let m = KMutex::new(0u32);
    for i in 0..20u32 {
        let mut g = m.lock();
        *g = i;
    }
    assert_eq!(*m.lock(), 19);
    moltos::serial_println!("[ok]");
}
