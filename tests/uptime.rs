//! Integration test: uptime counter increments after init and timer ticks.

#![no_std]
#![no_main]

use bootloader::{entry_point, BootInfo};
use core::panic::PanicInfo;
use chronos::{exit_qemu, init, uptime_ticks, QemuExitCode, test_panic_handler, serial_print, serial_println};

entry_point!(kernel_main);

fn kernel_main(_boot_info: &'static BootInfo) -> ! {
    serial_print!("uptime::test_uptime_increments...\t");
    
    init();

    // Ensure interrupts are enabled
    x86_64::instructions::interrupts::enable();

    let t0 = uptime_ticks();
    serial_println!("Initial uptime: {}", t0);
    
    // Wait for at least one timer interrupt to fire
    // Timer fires at 100 Hz, so this should complete quickly
    let mut iterations = 0;
    const MAX_ITERATIONS: u32 = 10_000_000; // Safety limit
    
    while iterations < MAX_ITERATIONS {
        use x86_64::instructions::interrupts::enable_and_hlt;
        enable_and_hlt(); // Atomically enable interrupts and halt
        iterations += 1;
        
        // Check if uptime has increased (timer interrupt fired)
        let t1 = uptime_ticks();
        if t1 > t0 {
            // Success: uptime increased, exit test
            serial_println!("[ok]");
            exit_qemu(QemuExitCode::Success);
            loop {}
        }
    }
    
    // If we get here, the test timed out
    let final_uptime = uptime_ticks();
    panic!("uptime did not increase after {} iterations (t0={}, t1={}, interrupts may not be firing)", 
           iterations, t0, final_uptime);
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    test_panic_handler(info)
}
