/***
 * src/thread/schedulers/eevdf.rs
 *
 * EEVDF (Earliest Eligible Virtual Deadline First) scheduler.
 * Each thread gets CPU time proportional to its weight. Lower virtual deadline
 * = higher priority to run next.
 */

use alloc::vec::Vec;
use super::super::context::ThreadContext;
use super::super::thread::{Thread, ThreadId};

const DEFAULT_WEIGHT: u64 = 1024;
const MIN_TIME_SLICE: u64 = 1; // ticks; at 100 Hz this is 10ms

struct ThreadMeta {
    vruntime: u64,  // normalized CPU time consumed (higher weight = slower growth)
    weight: u64,
    blocked: bool,  // true = waiting for a lock; skip in tick_prepare
}

impl ThreadMeta {
    fn new(_index: usize) -> Self {
        Self { vruntime: 0, weight: DEFAULT_WEIGHT, blocked: false }
    }

    // Virtual deadline: when this thread should next run.
    // Lower deadline = run sooner. Higher weight pushes deadline earlier.
    fn virtual_deadline(&self, time_slice: u64, sum_weights: u64) -> u64 {
        if self.weight == 0 { return u64::MAX; }
        self.vruntime + (time_slice * sum_weights) / self.weight
    }
}

pub struct EevdfScheduler {
    threads: Vec<Thread>,
    metadata: Vec<ThreadMeta>,
    current: Option<usize>,
    min_vruntime: u64,
    last_switch_tick: u64,
}

impl EevdfScheduler {
    pub const fn new() -> Self {
        Self {
            threads: Vec::new(),
            metadata: Vec::new(),
            current: None,
            min_vruntime: 0,
            last_switch_tick: 0,
        }
    }

    pub fn spawn(&mut self, thread: Thread) {
        let index = self.threads.len();
        self.threads.push(thread);
        self.metadata.push(ThreadMeta::new(index));
    }

    pub fn spawn_with_weight(&mut self, thread: Thread, weight: u64) {
        let index = self.threads.len();
        self.threads.push(thread);
        let mut meta = ThreadMeta::new(index);
        meta.weight = weight.max(1);
        self.metadata.push(meta);
    }

    pub fn len(&self) -> usize {
        self.threads.len()
    }

    pub fn current_thread_id(&self) -> Option<ThreadId> {
        self.current.map(|idx| self.threads[idx].id)
    }

    // Mark a thread runnable again. Called when a lock it was waiting on is released.
    pub fn unblock_thread(&mut self, id: ThreadId) {
        for (i, thread) in self.threads.iter().enumerate() {
            if thread.id == id {
                self.metadata[i].blocked = false;
                return;
            }
        }
    }

    // Pick the next thread to run and return (from_ctx, to_ctx) pointers.
    // Returns None if there are no threads or the current thread should keep running.
    pub fn tick_prepare(&mut self, bootstrap_ctx: *mut ThreadContext, current_tick: u64) -> Option<(*mut ThreadContext, *const ThreadContext)> {
        if self.threads.is_empty() { return None; }

        let time_slice = if self.current.is_some() {
            current_tick.saturating_sub(self.last_switch_tick).max(MIN_TIME_SLICE)
        } else {
            MIN_TIME_SLICE
        };

        // Update vruntime of the thread we're switching away from.
        if let Some(prev_idx) = self.current {
            let sum_weights: u64 = self.metadata.iter().filter(|m| !m.blocked).map(|m| m.weight).sum();
            if sum_weights > 0 {
                let weight = self.metadata[prev_idx].weight;
                let vruntime_delta = (time_slice * weight) / sum_weights;
                self.metadata[prev_idx].vruntime += vruntime_delta;
            }
        }

        // min_vruntime tracks over non-blocked threads so blocked threads don't skew fairness.
        self.min_vruntime = self.metadata.iter()
            .filter(|m| !m.blocked)
            .map(|m| m.vruntime)
            .min()
            .unwrap_or(self.min_vruntime);

        let sum_weights: u64 = self.metadata.iter().filter(|m| !m.blocked).map(|m| m.weight).sum();
        let lag_tolerance = sum_weights.max(1);

        // Find the eligible non-blocked thread with the earliest virtual deadline.
        let next_idx = self.metadata.iter()
            .enumerate()
            .filter(|(_, meta)| !meta.blocked && meta.vruntime <= self.min_vruntime.saturating_add(lag_tolerance))
            .min_by(|(_, a), (_, b)| {
                a.virtual_deadline(time_slice, sum_weights)
                    .cmp(&b.virtual_deadline(time_slice, sum_weights))
            })
            .map(|(idx, _)| idx)
            .or_else(|| {
                // All threads within lag tolerance are blocked; pick any non-blocked thread.
                self.metadata.iter()
                    .enumerate()
                    .find(|(_, meta)| !meta.blocked)
                    .map(|(idx, _)| idx)
            });

        let next_idx = match next_idx {
            Some(idx) => idx,
            None => return None,  // all threads blocked
        };

        let prev = self.current;

        // Skip switch if we're already on the best thread, unless enough time has passed
        // or we have no timer (current_tick == 0, keyboard-only mode).
        if prev == Some(next_idx) {
            if current_tick != 0 {
                let elapsed = current_tick.saturating_sub(self.last_switch_tick);
                if elapsed < MIN_TIME_SLICE { return None; }
            }
        }

        self.current = Some(next_idx);
        self.last_switch_tick = current_tick;

        let from_ctx: *mut ThreadContext = match prev {
            None    => bootstrap_ctx,
            Some(p) => self.threads[p].context_ptr(),
        };
        let to_ctx = self.threads[next_idx].context_ptr() as *const ThreadContext;
        Some((from_ctx, to_ctx))
    }

    // Mark the current thread as blocked and prepare a switch to the next runnable thread.
    // Used by sleeping mutexes to yield without spinning.
    pub fn block_current_and_prepare_switch(&mut self, bootstrap_ctx: *mut ThreadContext, current_tick: u64) -> Option<(*mut ThreadContext, *const ThreadContext)> {
        // Mark current thread blocked before picking the next one.
        if let Some(idx) = self.current {
            self.metadata[idx].blocked = true;
        }
        // Reuse tick_prepare — it already skips blocked threads and picks fairly.
        // Force a switch by temporarily clearing the same-thread guard.
        let saved_last = self.last_switch_tick;
        self.last_switch_tick = 0;  // make elapsed > MIN_TIME_SLICE to force a switch
        let result = self.tick_prepare(bootstrap_ctx, current_tick);
        if result.is_none() {
            // No runnable threads found; restore last_switch_tick so state is consistent.
            self.last_switch_tick = saved_last;
        }
        result
    }
}
