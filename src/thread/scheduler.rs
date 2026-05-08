/***
 * src/thread/scheduler.rs
 *
 * Global scheduler. Wraps EevdfScheduler and owns the bootstrap context —
 * the saved register state of kernel_main used as the "from" slot on the
 * first context switch.
 *
 * SCHEDULER is an IrqMutex so every access automatically disables interrupts.
 * This prevents the single-core deadlock where a thread holds the lock, gets
 * timer-interrupted, and the handler spins on it forever.
 */

use crate::sync::IrqMutex;
use super::context::ThreadContext;
use super::schedulers::eevdf::EevdfScheduler;
use super::thread::{Thread, ThreadId};

pub static SCHEDULER: IrqMutex<Scheduler> = IrqMutex::new(Scheduler::new());

pub struct Scheduler {
    eevdf: EevdfScheduler,
    bootstrap_ctx: ThreadContext,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            eevdf: EevdfScheduler::new(),
            bootstrap_ctx: ThreadContext {
                rsp: 0, rip: 0,
                rbx: 0, rbp: 0, r12: 0, r13: 0, r14: 0, r15: 0,
                rax: 0, rcx: 0, rdx: 0, rsi: 0, rdi: 0,
                r8:  0, r9:  0, r10: 0, r11: 0,
                rflags: 0x202,
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

    pub fn tick_prepare(&mut self, current_tick: u64) -> Option<(*mut ThreadContext, *const ThreadContext)> {
        let bootstrap_ptr = &mut self.bootstrap_ctx as *mut ThreadContext;
        self.eevdf.tick_prepare(bootstrap_ptr, current_tick)
    }

    pub fn block_current_and_prepare_switch(&mut self, current_tick: u64) -> Option<(*mut ThreadContext, *const ThreadContext)> {
        let bootstrap_ptr = &mut self.bootstrap_ctx as *mut ThreadContext;
        self.eevdf.block_current_and_prepare_switch(bootstrap_ptr, current_tick)
    }
}
