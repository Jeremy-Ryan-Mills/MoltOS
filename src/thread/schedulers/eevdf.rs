//! EEVDF (Earliest Eligible Virtual Deadline First) scheduler.
//!
//! EEVDF is a fair-share scheduler that gives each thread a proportional share
//! of CPU time based on its weight. Threads are scheduled by earliest eligible
//! virtual deadline.

use alloc::vec::Vec;

use super::super::context::ThreadContext;
use super::super::thread::Thread;

/// Default weight for a thread (normal priority).
const DEFAULT_WEIGHT: u64 = 1024;

/// Minimum time slice in timer ticks before we consider switching.
const MIN_TIME_SLICE: u64 = 1;

/// Thread scheduling metadata for EEVDF.
struct ThreadMeta {
    /// Virtual runtime: tracks how much CPU time this thread has used,
    /// normalized by its weight.
    vruntime: u64,
    /// Weight/priority: higher weight = more CPU time. Default is 1024.
    weight: u64,
    /// Index into the threads vector.
    index: usize,
}

impl ThreadMeta {
    fn new(index: usize) -> Self {
        Self {
            vruntime: 0,
            weight: DEFAULT_WEIGHT,
            index,
        }
    }

    /// Computes the virtual deadline for this thread given a time slice.
    /// deadline = vruntime + (time_slice * sum_weights / weight)
    fn virtual_deadline(&self, time_slice: u64, sum_weights: u64) -> u64 {
        if self.weight == 0 {
            return u64::MAX;
        }
        // Virtual deadline: when this thread should be scheduled next.
        // Higher weight threads get earlier deadlines (more CPU time).
        self.vruntime + (time_slice * sum_weights) / self.weight
    }
}

/// EEVDF scheduler: fair-share scheduling based on virtual deadlines.
pub struct EevdfScheduler {
    threads: Vec<Thread>,
    metadata: Vec<ThreadMeta>,
    /// Index of the thread that is currently running. None = we're still in bootstrap.
    current: Option<usize>,
    /// Global minimum vruntime (for eligibility).
    min_vruntime: u64,
    /// Last tick when we switched (to compute time slice).
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

    /// Adds a new thread with default weight.
    pub fn spawn(&mut self, thread: Thread) {
        let index = self.threads.len();
        self.threads.push(thread);
        self.metadata.push(ThreadMeta::new(index));
    }

    /// Adds a new thread with a custom weight (higher = more CPU time).
    pub fn spawn_with_weight(&mut self, thread: Thread, weight: u64) {
        let index = self.threads.len();
        self.threads.push(thread);
        let mut meta = ThreadMeta::new(index);
        meta.weight = weight.max(1); // Ensure weight >= 1
        self.metadata.push(meta);
    }

    /// Number of threads (excluding bootstrap).
    pub fn len(&self) -> usize {
        self.threads.len()
    }

    /// Prepares a context switch: picks the thread with the earliest eligible
    /// virtual deadline and returns the two context pointers.
    pub fn tick_prepare(&mut self, bootstrap_ctx: *mut ThreadContext, current_tick: u64) -> Option<(*mut ThreadContext, *const ThreadContext)> {
        let n = self.threads.len();
        if n == 0 {
            return None;
        }

        // Compute time slice since last switch.
        let time_slice = if let Some(prev_idx) = self.current {
            let elapsed = current_tick.saturating_sub(self.last_switch_tick);
            elapsed.max(MIN_TIME_SLICE)
        } else {
            // First switch: use minimum slice.
            MIN_TIME_SLICE
        };

        // Update vruntime of the thread we're switching away from.
        if let Some(prev_idx) = self.current {
            let sum_weights: u64 = self.metadata.iter().map(|m| m.weight).sum();
            if sum_weights > 0 {
                let weight = self.metadata[prev_idx].weight;
                // vruntime += (time_slice * weight) / sum_weights
                // This normalizes by weight so higher-weight threads advance vruntime slower.
                let vruntime_delta = (time_slice * weight) / sum_weights;
                self.metadata[prev_idx].vruntime += vruntime_delta;
            }
        }

        // Update min_vruntime to be the minimum of all vruntimes.
        self.min_vruntime = self.metadata.iter()
            .map(|m| m.vruntime)
            .min()
            .unwrap_or(0);

        // Find the thread with the earliest eligible virtual deadline.
        // A thread is eligible if its vruntime is not too far behind min_vruntime.
        // We use a lag tolerance based on sum of weights.
        let sum_weights: u64 = self.metadata.iter().map(|m| m.weight).sum();
        let lag_tolerance = sum_weights.max(1); // Allow threads to lag by at most this much
        
        let next_idx = self.metadata.iter()
            .enumerate()
            .filter(|(_, meta)| {
                // Eligible if vruntime is within lag_tolerance of min_vruntime
                meta.vruntime <= self.min_vruntime.saturating_add(lag_tolerance)
            })
            .min_by(|(_, a), (_, b)| {
                let deadline_a = a.virtual_deadline(time_slice, sum_weights);
                let deadline_b = b.virtual_deadline(time_slice, sum_weights);
                deadline_a.cmp(&deadline_b)
            })
            .map(|(idx, _)| idx)
            .unwrap_or(0); // Fallback to first thread if none eligible

        let prev = self.current;
        self.current = Some(next_idx);
        self.last_switch_tick = current_tick;

        let from_ctx: *mut ThreadContext = match prev {
            None => bootstrap_ctx,
            Some(p) => self.threads[p].context_ptr(),
        };
        let to_ctx = self.threads[next_idx].context_ptr() as *const ThreadContext;
        Some((from_ctx, to_ctx))
    }
}
