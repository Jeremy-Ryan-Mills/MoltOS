/***
 * src/thread/scheduler.rs
 *
 * Global scheduler. Wraps EevdfScheduler and owns the bootstrap context —
 * the saved register state of kernel_main used as the "from" slot on the
 * first context switch.
 */

use spin::Mutex;
use super::context::ThreadContext;
use super::schedulers::eevdf::EevdfScheduler;
use super::thread::{Thread, ThreadId};

pub static SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());

pub struct Scheduler {
    eevdf: EevdfScheduler,
    // Saved state for kernel_main. The context switch writes here when leaving
    // bootstrap; the scheduler reads it back when returning to bootstrap.
    bootstrap_ctx: ThreadContext,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            eevdf: EevdfScheduler::new(),
            bootstrap_ctx: ThreadContext {
                rsp: 0, rip: 0, rbx: 0, rbp: 0,
                r12: 0, r13: 0, r14: 0, r15: 0,
            },
        }
    }

    pub fn spawn(&mut self, thread: Thread) {
        self.eevdf.spawn(thread);
    }

    pub fn spawn_with_weight(&mut self, thread: Thread, weight: u64) {
        self.eevdf.spawn_with_weight(thread, weight);
    }

    pub fn len(&self) -> usize {
        self.eevdf.len()
    }

    pub fn current_thread_id(&self) -> Option<ThreadId> {
        self.eevdf.current_thread_id()
    }

    pub fn unblock_thread(&mut self, id: ThreadId) {
        self.eevdf.unblock_thread(id);
    }

    // Picks the next thread and returns (from_ctx, to_ctx) pointers.
    // The caller must drop the lock before calling context_switch.
    pub fn tick_prepare(&mut self, current_tick: u64) -> Option<(*mut ThreadContext, *const ThreadContext)> {
        let bootstrap_ptr = &mut self.bootstrap_ctx as *mut ThreadContext;
        self.eevdf.tick_prepare(bootstrap_ptr, current_tick)
    }

    // Mark the current thread blocked and pick the next runnable thread.
    // The caller must drop the lock before calling context_switch.
    pub fn block_current_and_prepare_switch(&mut self, current_tick: u64) -> Option<(*mut ThreadContext, *const ThreadContext)> {
        let bootstrap_ptr = &mut self.bootstrap_ctx as *mut ThreadContext;
        self.eevdf.block_current_and_prepare_switch(bootstrap_ptr, current_tick)
    }
}
