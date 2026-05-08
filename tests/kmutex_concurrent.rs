/***
 * tests/kmutex_concurrent.rs
 *
 * Integration test: multiple kernel threads contend on a KMutex.
 * Spawns WORKERS threads each incrementing a shared counter ITERS times,
 * then a verifier thread that waits for all workers to finish and asserts
 * the final count matches WORKERS * ITERS exactly (no lost updates).
 *
 * Uses harness=false so we can call enter_scheduler().
 */

#![no_std]
#![no_main]

extern crate alloc;

use bootloader::{entry_point, BootInfo};
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicU32, Ordering};
use moltos::sync::KMutex;
use moltos::thread::{Thread, SCHEDULER};
use moltos::{exit_qemu, QemuExitCode};

entry_point!(main);

const WORKERS: u32 = 3;
const ITERS: u32 = 200;

static COUNTER: KMutex<u32> = KMutex::new(0);
// Counts workers that have finished all their increments.
static DONE: AtomicU32 = AtomicU32::new(0);

fn worker() {
    for _ in 0..ITERS {
        let mut g = COUNTER.lock();
        *g += 1;
    }
    DONE.fetch_add(1, Ordering::Release);
    loop { x86_64::instructions::hlt(); }
}

fn verifier() {
    // Spin until all workers report done.
    while DONE.load(Ordering::Acquire) < WORKERS {
        core::hint::spin_loop();
    }

    let final_count = *COUNTER.lock();
    let expected = WORKERS * ITERS;
    if final_count == expected {
        moltos::serial_println!("kmutex_concurrent: counter={} [ok]", final_count);
        exit_qemu(QemuExitCode::Success);
    } else {
        moltos::serial_println!(
            "kmutex_concurrent: FAILED counter={} expected={}",
            final_count, expected
        );
        exit_qemu(QemuExitCode::Failed);
    }
    loop {}
}

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

    {
        let mut sched = SCHEDULER.lock();
        for _ in 0..WORKERS {
            sched.spawn(Thread::new(worker));
        }
        sched.spawn(Thread::new(verifier));
    }

    moltos::thread::enter_scheduler();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    moltos::test_panic_handler(info)
}
