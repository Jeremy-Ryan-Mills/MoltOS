//! Round-robin scheduler: threads run in a fixed circular order.

use alloc::vec::Vec;

use super::super::context::ThreadContext;
use super::super::thread::Thread;

/// Round-robin scheduler: list of threads and current index.
pub struct RoundRobinScheduler {
    threads: Vec<Thread>,
    /// Index of the thread that is currently running. None = we're still in bootstrap.
    current: Option<usize>,
}

impl RoundRobinScheduler {
    pub const fn new() -> Self {
        Self {
            threads: Vec::new(),
            current: None,
        }
    }

    /// Adds a new thread; it will be scheduled in round-robin order.
    pub fn spawn(&mut self, thread: Thread) {
        self.threads.push(thread);
    }

    /// Adds a new thread with a custom weight (ignored in round-robin).
    pub fn spawn_with_weight(&mut self, thread: Thread, _weight: u64) {
        self.threads.push(thread);
    }

    /// Number of threads (excluding bootstrap).
    pub fn len(&self) -> usize {
        self.threads.len()
    }

    /// Prepares a context switch: picks the next thread and returns the two
    /// context pointers. The caller must release the lock and then call
    /// `context_switch(from, to)` so we don't hold the lock across the switch.
    /// 
    /// `current_tick` is ignored in round-robin scheduling.
    pub fn tick_prepare(&mut self, bootstrap_ctx: *mut ThreadContext, _current_tick: u64) -> Option<(*mut ThreadContext, *const ThreadContext)> {
        let n = self.threads.len();
        if n == 0 {
            return None;
        }

        let prev = self.current;
        let next = match prev {
            None => 0,
            Some(i) => (i + 1) % n,
        };
        self.current = Some(next);

        let from_ctx: *mut ThreadContext = match prev {
            None => bootstrap_ctx,
            Some(p) => self.threads[p].context_ptr(),
        };
        let to_ctx = self.threads[next].context_ptr() as *const ThreadContext;
        Some((from_ctx, to_ctx))
    }
}
