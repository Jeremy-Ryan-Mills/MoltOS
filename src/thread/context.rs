// Saved CPU register state for a kernel thread.

use core::arch::asm;

// Layout must match the offsets used in the asm below.
// Order: rsp (0), rip (8), rbx (16), rbp (24), r12 (32), r13 (40), r14 (48), r15 (56)
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct ThreadContext {
    pub rsp: u64,
    pub rip: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
}

// Save current context into `old`, restore from `new`, jump to new thread.
// Enables interrupts (sti) so the new thread receives IRQs.
// Safety: both pointers must be valid; `new` must hold a properly initialized context.
#[inline(never)]
pub unsafe fn context_switch(old: *mut ThreadContext, new: *const ThreadContext) {
    unsafe {
        asm!(
            "cli",
            "mov [rdi + 0],  rsp",
            "mov rax,        [rsp]",
            "mov [rdi + 8],  rax",
            "mov [rdi + 16], rbx",
            "mov [rdi + 24], rbp",
            "mov [rdi + 32], r12",
            "mov [rdi + 40], r13",
            "mov [rdi + 48], r14",
            "mov [rdi + 56], r15",
            "mov rsp,        [rsi + 0]",
            "mov rbx,        [rsi + 16]",
            "mov rbp,        [rsi + 24]",
            "mov r12,        [rsi + 32]",
            "mov r13,        [rsi + 40]",
            "mov r14,        [rsi + 48]",
            "mov r15,        [rsi + 56]",
            "sti",
            "ret",
            in("rdi") old,
            in("rsi") new,
            options(nostack)
        );
    }
}

// Restore from `new` without saving current context.
// Used from interrupt handlers where the interrupted thread's state was already
// written directly into its context struct before calling this.
// Safety: `new` must hold a properly initialized context.
#[inline(never)]
pub unsafe fn context_switch_to(new: *const ThreadContext) {
    unsafe {
        asm!(
            "mov rsp, [rdi + 0]",
            "mov rbx, [rdi + 16]",
            "mov rbp, [rdi + 24]",
            "mov r12, [rdi + 32]",
            "mov r13, [rdi + 40]",
            "mov r14, [rdi + 48]",
            "mov r15, [rdi + 56]",
            "sti",
            "ret",
            in("rdi") new,
            options(nostack)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::ThreadContext;

    #[test_case]
    fn test_context_default() {
        let ctx = ThreadContext::default();
        assert_eq!(ctx.rsp, 0);
        assert_eq!(ctx.rip, 0);
    }

    #[test_case]
    fn test_context_clone() {
        let ctx1 = ThreadContext { rsp: 0x1000, rip: 0x2000, rbx: 0x3000, rbp: 0x4000,
                                   r12: 0x5000, r13: 0x6000, r14: 0x7000, r15: 0x8000 };
        let ctx2 = ctx1;
        assert_eq!(ctx2.rsp, 0x1000);
        assert_eq!(ctx2.rip, 0x2000);
    }
}
