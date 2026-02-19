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
    loop {
        let switch = {
            let mut sched = SCHEDULER.lock();
            let current_tick = crate::uptime_ticks();
            sched.tick_prepare(current_tick)
        };
        if let Some((from_ctx, to_ctx)) = switch {
            unsafe {
                context_switch(from_ctx, to_ctx);
            }
        }
    }
}
