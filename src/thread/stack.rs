use alloc::boxed::Box;
use super::context::ThreadContext;

pub const STACK_SIZE: usize = 16 * 1024;
const STACK_ALIGN: usize = 16;

pub struct KernelStack {
    storage: Box<[u8; STACK_SIZE]>,
}

impl KernelStack {
    pub fn new() -> Self {
        Self { storage: Box::new([0u8; STACK_SIZE]) }
    }

    // Highest address of the stack (stack grows downward), aligned for ABI calls.
    pub fn top(&self) -> u64 {
        let end = self.storage.as_ptr() as u64 + STACK_SIZE as u64;
        (end & !(STACK_ALIGN as u64)) - 8
    }

    // Set up ctx for first-time entry to entry_point.
    // Writes entry_point at the top of the stack so the first `ret` jumps there.
    pub fn init_context(&mut self, ctx: &mut ThreadContext, entry_point: usize) {
        let top = self.top();
        unsafe { (top as *mut u64).write(entry_point as u64); }
        ctx.rsp = top;
        ctx.rip = entry_point as u64;
        ctx.rbx = 0; ctx.rbp = 0;
        ctx.r12 = 0; ctx.r13 = 0;
        ctx.r14 = 0; ctx.r15 = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::{KernelStack, STACK_SIZE, STACK_ALIGN};
    use crate::thread::context::ThreadContext;

    #[test_case]
    fn test_stack_allocation() {
        let stack = KernelStack::new();
        assert!(stack.top() > 0);
        assert_eq!(stack.top() % STACK_ALIGN as u64, 8);
    }

    #[test_case]
    fn test_stack_init_context() {
        let mut stack = KernelStack::new();
        let mut ctx = ThreadContext::default();
        stack.init_context(&mut ctx, 0x12345678);
        assert_eq!(ctx.rsp, stack.top());
        assert_eq!(ctx.rip, 0x12345678);
        unsafe { assert_eq!(*(ctx.rsp as *const u64), 0x12345678); }
    }

    #[test_case]
    fn test_stack_alignment() {
        let stack = KernelStack::new();
        assert_eq!((stack.top() + 8) % STACK_ALIGN as u64, 0);
    }
}
