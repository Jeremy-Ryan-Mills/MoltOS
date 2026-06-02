use alloc::vec::Vec;

use super::super::context::ThreadContext;
use super::super::thread::Thread;

pub struct RoundRobinScheduler {
    threads: Vec<Thread>,
    current: Option<usize>,
}

impl RoundRobinScheduler {
    pub const fn new() -> Self {
        Self {
            threads: Vec::new(),
            current: None,
        }
    }

    pub fn spawn(&mut self, thread: Thread) {
        self.threads.push(thread);
    }

    pub fn spawn_with_weight(&mut self, thread: Thread, _weight: u64) {
        self.threads.push(thread);
    }

    pub fn len(&self) -> usize {
        self.threads.len()
    }

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
