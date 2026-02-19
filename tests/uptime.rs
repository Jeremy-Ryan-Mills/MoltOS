//! Integration test: uptime counter increments after init and timer ticks.

#![no_std]
#![no_main]

use bootloader::{entry_point, BootInfo};
use core::panic::PanicInfo;
use chronos::{exit_qemu, init, uptime_ticks, QemuExitCode, test_panic_handler};

entry_point!(kernel_main);

fn kernel_main(_boot_info: &'static BootInfo) -> ! {
    init();

    let t0 = uptime_ticks();
    for _ in 0..5_000_000 {
        x86_64::instructions::hlt();
    }
    let t1 = uptime_ticks();

    assert!(t1 > t0, "uptime should increase ({} -> {})", t0, t1);
    exit_qemu(QemuExitCode::Success);
    loop {}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    test_panic_handler(info)
}
