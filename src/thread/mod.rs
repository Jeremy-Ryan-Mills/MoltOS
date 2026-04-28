/***
 * src/thread/mod.rs
 *
 * Preemptive kernel threads. Timer interrupts switch threads via context_switch.
 */

pub mod context;
pub mod scheduler;
pub mod schedulers;
pub mod stack;
pub mod thread;

pub use context::context_switch;
pub use scheduler::{Scheduler, SCHEDULER};
pub use thread::{Thread, ThreadId};

// Enter the scheduler. Does not return. Switches from bootstrap (kernel_main)
// to the first thread; subsequent switches happen via timer/keyboard IRQs.
pub fn enter_scheduler() -> ! {
    loop {
        let switch = {
            let mut sched = SCHEDULER.lock();
            sched.tick_prepare(crate::uptime_ticks())
        };

        if let Some((from_ctx, to_ctx)) = switch {
            unsafe {
                if to_ctx.is_null() || (*to_ctx).rsp == 0 {
                    crate::serial_println!("ERROR: null/zeroed context in enter_scheduler");
                    crate::hlt_loop();
                }
                context_switch(from_ctx, to_ctx);
            }
        }
        x86_64::instructions::interrupts::enable_and_hlt();
    }
}
