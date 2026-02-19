//! Scheduler wrapper that delegates to the active scheduler implementation.

use spin::Mutex;

use super::context::ThreadContext;
use super::schedulers::Scheduler as SchedulerImpl;

/// Context saved when we leave the bootstrap (kernel_main) to enter a thread.
static mut BOOTSTRAP_CTX: ThreadContext = ThreadContext {
    rsp: 0,
    rip: 0,
    rbx: 0,
    rbp: 0,
    r12: 0,
    r13: 0,
    r14: 0,
    r15: 0,
};

/// Global scheduler state (uses EEVDF by default).
pub static SCHEDULER: Mutex<Scheduler> = Mutex::new(Scheduler::new());

/// Scheduler wrapper that provides a unified interface.
pub struct Scheduler {
    inner: SchedulerImpl,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            inner: SchedulerImpl::new(),
        }
    }

    /// Adds a new thread with default weight.
    pub fn spawn(&mut self, thread: super::thread::Thread) {
        self.inner.spawn(thread);
    }

    /// Adds a new thread with a custom weight (EEVDF only).
    pub fn spawn_with_weight(&mut self, thread: super::thread::Thread, weight: u64) {
        self.inner.spawn_with_weight(thread, weight);
    }

    /// Number of threads (excluding bootstrap).
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Prepares a context switch: picks the next thread and returns the two
    /// context pointers. The caller must release the lock and then call
    /// `context_switch(from, to)` so we don't hold the lock across the switch.
    pub fn tick_prepare(&mut self, current_tick: u64) -> Option<(*mut ThreadContext, *const ThreadContext)> {
        let bootstrap_ctx = unsafe { &raw mut BOOTSTRAP_CTX };
        self.inner.tick_prepare(bootstrap_ctx, current_tick)
    }
}
