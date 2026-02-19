//! Per-thread kernel stack.

use alloc::boxed::Box;

use super::context::ThreadContext;

/// Default kernel stack size per thread (16 KiB).
pub const STACK_SIZE: usize = 16 * 1024;

/// Stack must be 16-byte aligned at the boundary the CPU uses for `call`.
const STACK_ALIGN: usize = 16;

/// A fixed-size kernel stack for one thread.
pub struct KernelStack {
    storage: Box<[u8; STACK_SIZE]>,
}

impl KernelStack {
    /// Allocates a new kernel stack.
    pub fn new() -> Self {
        Self {
            storage: Box::new([0u8; STACK_SIZE]),
        }
    }

    /// Returns the top of the stack (highest address; stack grows down).
    /// Aligned to STACK_ALIGN.
    pub fn top(&self) -> u64 {
        let start = self.storage.as_ptr() as u64;
        let end = start + STACK_SIZE as u64;
        (end & !(STACK_ALIGN as u64)) - 8
    }

    /// Sets up `ctx` for first-time entry to `entry_point`.
    /// Writes the entry point address at the top of the stack so that
    /// when we context-switch to this thread, `ret` jumps to `entry_point`.
    pub fn init_context(&mut self, ctx: &mut ThreadContext, entry_point: usize) {
        let top = self.top();
        unsafe {
            let rip_slot = top as *mut u64;
            rip_slot.write(entry_point as u64);
        }
        ctx.rsp = top;
        ctx.rip = entry_point as u64;
        ctx.rbx = 0;
        ctx.rbp = 0;
        ctx.r12 = 0;
        ctx.r13 = 0;
        ctx.r14 = 0;
        ctx.r15 = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::{KernelStack, STACK_SIZE, STACK_ALIGN};
    use crate::thread::context::ThreadContext;

    #[test_case]
    fn test_stack_allocation() {
        let stack = KernelStack::new();
        let top = stack.top();
        assert!(top > 0);
        assert_eq!(top % STACK_ALIGN as u64, 8); // Top - 8 should be aligned
    }

    #[test_case]
    fn test_stack_init_context() {
        let mut stack = KernelStack::new();
        let mut ctx = ThreadContext::default();
        let entry_point = 0x12345678usize;
        
        stack.init_context(&mut ctx, entry_point);
        
        assert_eq!(ctx.rsp, stack.top());
        assert_eq!(ctx.rip, entry_point as u64);
        assert_eq!(ctx.rbx, 0);
        
        // Verify entry point was written to stack
        unsafe {
            let rip_slot = ctx.rsp as *const u64;
            assert_eq!(*rip_slot, entry_point as u64);
        }
    }

    #[test_case]
    fn test_stack_alignment() {
        let stack = KernelStack::new();
        let top = stack.top();
        // Stack top should be aligned to 16 bytes minus 8 (for the return address slot)
        assert_eq!((top + 8) % STACK_ALIGN as u64, 0);
    }
}
