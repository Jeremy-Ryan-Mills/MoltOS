use alloc::boxed::Box;
use alloc::vec;
use super::context::ThreadContext;

pub const STACK_SIZE: usize = 16 * 1024;
const STACK_ALIGN: usize = 16;

const CANARY: u64 = 0xDEAD_C0DE_DEAD_C0DE;

pub struct KernelStack {
    storage: Box<[u8]>,
}

impl KernelStack {
    pub fn new() -> Self {
        Self::with_size(STACK_SIZE)
    }

    pub fn with_size(size: usize) -> Self {
        let mut s = Self { storage: vec![0u8; size].into_boxed_slice() };
        s.write_canary();
        s
    }

    fn write_canary(&mut self) {
        if self.storage.len() >= 8 {
            let ptr = self.storage.as_mut_ptr() as *mut u64;
            unsafe { ptr.write(CANARY); }
        }
    }

    pub fn check_canary(&self) -> bool {
        if self.storage.len() < 8 { return true; }
        let ptr = self.storage.as_ptr() as *const u64;
        unsafe { ptr.read() == CANARY }
    }

    // Highest address of the stack (stack grows downward), aligned for ABI calls.
    // We want RSP ≡ 8 (mod 16) at function entry (x86_64 ABI).
    // context_switch_to uses `ret` which pops 8 bytes, so we place the entry point
    // at RSP = 16n-16; after `ret`, RSP = 16n-8 as the ABI requires.
    pub fn top(&self) -> u64 {
        let end = self.storage.as_ptr() as u64 + self.storage.len() as u64;
        (end & !(STACK_ALIGN as u64)) - 16
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
        // top is 16-byte aligned; after ret pops 8 bytes, RSP = top+8 (16n-8, ABI entry)
        assert_eq!(stack.top() % STACK_ALIGN as u64, 0);
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
        // After ret from top, RSP = top+8. We want (RSP+8) % 16 == 0 (x86-64 ABI at call sites).
        assert_eq!((stack.top() + 16) % STACK_ALIGN as u64, 0);
    }
}
