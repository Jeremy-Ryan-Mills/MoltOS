//! x86_64 implementation of the arch interface.
//!
//! Uses GDT/TSS, IDT, PIC, PIT, port I/O, and ISA debug exit.

use x86_64::instructions;

pub fn init() {
    crate::gdt::init();
    crate::interrupts::init_idt();
    unsafe { crate::interrupts::PICS.lock().initialize() };
    instructions::interrupts::enable();
}

pub fn hlt_loop() -> ! {
    loop {
        instructions::hlt();
    }
}

pub fn exit_qemu(code: crate::QemuExitCode) {
    use x86_64::instructions::port::Port;
    unsafe {
        let mut port = Port::new(0xf4);
        port.write(code as u32);
    }
}
