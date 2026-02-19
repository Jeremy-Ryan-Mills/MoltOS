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
    // First, ensure we switch from bootstrap to the first thread
    // This should always succeed if there's at least one thread
    let mut first_switch_done = false;
    
    loop {
        let switch = {
            let mut sched = SCHEDULER.lock();
            let current_tick = crate::uptime_ticks();
            sched.tick_prepare(current_tick)
        };
        if let Some((from_ctx, to_ctx)) = switch {
            first_switch_done = true;
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
            // No switch needed - this can happen if scheduler decides not to switch
            // On first call, this shouldn't happen (we should switch from bootstrap)
            if !first_switch_done {
                crate::serial_println!("ERROR: tick_prepare returned None on first call - no threads?");
                crate::hlt_loop();
            }
            // Without PIT, we need keyboard interrupts to trigger switches, so use enable_and_hlt
            // to allow keyboard interrupts to wake us
            use x86_64::instructions::interrupts::enable_and_hlt;
            enable_and_hlt();
        }
    }
}
