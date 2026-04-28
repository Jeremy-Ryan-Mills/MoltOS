/***
 * src/thread/schedulers/eevdf.rs
 *
 * EEVDF (Earliest Eligible Virtual Deadline First) scheduler.
 * Each thread gets CPU time proportional to its weight. Lower virtual deadline
 * = higher priority to run next.
 */

use alloc::vec::Vec;
use super::super::context::ThreadContext;
use super::super::thread::Thread;

const DEFAULT_WEIGHT: u64 = 1024;
const MIN_TIME_SLICE: u64 = 1; // ticks; at 100 Hz this is 10ms

struct ThreadMeta {
    vruntime: u64,  // normalized CPU time consumed (higher weight = slower growth)
    weight: u64,
}

impl ThreadMeta {
    fn new(_index: usize) -> Self {
        Self { vruntime: 0, weight: DEFAULT_WEIGHT }
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

    // Pick the next thread to run and return (from_ctx, to_ctx) pointers.
    // Returns None if there are no threads or the current thread should keep running.
    pub fn tick_prepare(&mut self, bootstrap_ctx: *mut ThreadContext, current_tick: u64) -> Option<(*mut ThreadContext, *const ThreadContext)> {
        let n = self.threads.len();
        if n == 0 { return None; }

        let time_slice = if self.current.is_some() {
            current_tick.saturating_sub(self.last_switch_tick).max(MIN_TIME_SLICE)
        } else {
            MIN_TIME_SLICE
        };

        // Update vruntime of the thread we're switching away from.
        if let Some(prev_idx) = self.current {
            let sum_weights: u64 = self.metadata.iter().map(|m| m.weight).sum();
            if sum_weights > 0 {
                let weight = self.metadata[prev_idx].weight;
                let vruntime_delta = (time_slice * weight) / sum_weights;
                self.metadata[prev_idx].vruntime += vruntime_delta;
            }
        }

        self.min_vruntime = self.metadata.iter().map(|m| m.vruntime).min().unwrap_or(0);

        let sum_weights: u64 = self.metadata.iter().map(|m| m.weight).sum();
        let lag_tolerance = sum_weights.max(1);

        // Find the eligible thread with the earliest virtual deadline.
        let next_idx = self.metadata.iter()
            .enumerate()
            .filter(|(_, meta)| meta.vruntime <= self.min_vruntime.saturating_add(lag_tolerance))
            .min_by(|(_, a), (_, b)| {
                a.virtual_deadline(time_slice, sum_weights)
                    .cmp(&b.virtual_deadline(time_slice, sum_weights))
            })
            .map(|(idx, _)| idx)
            .unwrap_or(0);

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
}
