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

// Enter the scheduler from bootstrap context. Does not return.
// Switches to the first runnable thread; subsequent switches happen via timer IRQs.
pub fn enter_scheduler() -> ! {
    loop {
        // Hold the scheduler lock only long enough to pick the next thread, then
        // release the spinlock while keeping interrupts disabled until context_switch.
        // If interrupts were re-enabled before context_switch, a timer could fire,
        // update scheduler.current to a different thread, and then our context_switch
        // would set the wrong thread as "current" after the switch.
        let mut guard  = SCHEDULER.lock();
        let switch = guard.tick_prepare(crate::uptime_ticks());
        guard.release_without_irq_restore();

        if let Some((from, to)) = switch {
            unsafe {
                if to.is_null() || (*to).rsp == 0 {
                    crate::serial_println!("ERROR: null/zeroed context in enter_scheduler");
                    crate::hlt_loop();
                }
                context_switch(from, to);
            }
        } else {
            x86_64::instructions::interrupts::enable_and_hlt();
        }
    }
}

// Block the current thread and yield to the next runnable one.
// context_switch re-enables interrupts via sti before ret.
pub unsafe fn block_and_yield() {
    let mut guard  = SCHEDULER.lock();
    let switch = guard.block_current_and_prepare_switch(crate::uptime_ticks());
    guard.release_without_irq_restore();

    match switch {
        Some((from, to)) => unsafe { context_switch(from, to); },
        None => {
            crate::serial_println!("DEADLOCK: block_and_yield with no runnable threads");
            loop { x86_64::instructions::interrupts::enable_and_hlt(); }
        }
    }
}
