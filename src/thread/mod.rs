//! Preemptive kernel threads.
//!
//! Threads are scheduled round-robin on each timer tick. Each thread has its
//! own kernel stack and saved context.

pub mod context;
pub mod scheduler;
pub mod schedulers;
pub mod stack;
pub mod thread;

pub use context::context_switch;
pub use scheduler::{Scheduler, SCHEDULER};
pub use thread::{Thread, ThreadId};

/// Enters the thread scheduler. Does not return. The first switch goes from
/// bootstrap (current execution) to the first thread. Call after spawning
/// at least one thread.
pub fn enter_scheduler() -> ! {
    crate::serial_println!("[enter_scheduler] Starting scheduler loop");
    loop {
        let switch = {
            let mut sched = SCHEDULER.lock();
            let current_tick = crate::uptime_ticks();
            let result = sched.tick_prepare(current_tick);
            crate::serial_println!("[enter_scheduler] tick_prepare returned {} threads, tick={}", sched.len(), current_tick);
            result
        };
        if let Some((from_ctx, to_ctx)) = switch {
            crate::serial_println!("[enter_scheduler] About to context switch");
            unsafe {
                // Safety checks: ensure pointers are valid and thread has a valid stack.
                if to_ctx.is_null() {
                    crate::serial_println!("ERROR: Attempting to switch to null context!");
                    crate::hlt_loop();
                }
                let target_rsp = (*to_ctx).rsp;
                if target_rsp == 0 {
                    crate::serial_println!("ERROR: Attempting to switch to thread with rsp=0!");
                    crate::hlt_loop();
                }
                context_switch(from_ctx, to_ctx);
                // If we return here, we're back in bootstrap (shouldn't happen)
                crate::serial_println!("[scheduler] Returned from context_switch (unexpected!)");
            }
        } else {
            // No threads to switch to - halt and wait for timer interrupt
            x86_64::instructions::interrupts::enable();
            x86_64::instructions::hlt();
        }
    }
}
