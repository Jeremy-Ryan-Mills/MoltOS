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

// Block the current thread and yield to the next runnable one.
//
// Must be called with interrupts already disabled — the caller is responsible
// for disabling them before adding itself to a waiter list to prevent a lost
// wakeup race. Interrupts are re-enabled inside context_switch (via sti).
//
// Not safe from bootstrap context or interrupt handlers.
pub unsafe fn block_and_yield() {
    let switch = {
        let mut sched = SCHEDULER.lock();
        sched.block_current_and_prepare_switch(crate::uptime_ticks())
    };

    match switch {
        Some((from, to)) => unsafe { context_switch(from, to); },
        None => {
            // All threads are blocked — deadlock.
            crate::serial_println!("DEADLOCK: block_and_yield with no runnable threads");
            loop { x86_64::instructions::interrupts::enable_and_hlt(); }
        }
    }
}
