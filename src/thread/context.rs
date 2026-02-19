//! Saved CPU context for thread context switching.

use core::arch::asm;

/// Saved callee-saved registers + rsp/rip for x86_64 context switch.
///
/// Layout must match the assembly in `context_switch`. Order: rsp, rip, then
/// rbx, rbp, r12, r13, r14, r15 (callee-saved per ABI).
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

/// Saves the current context into `old`, restores context from `new`, and
/// jumps to the new context's instruction pointer.
///
/// When it "returns", we're in the new context (so we don't return in the
/// normal sense when switching away). Enables interrupts (sti) so the new
/// thread receives timer/keyboard IRQs.
///
/// # Safety
/// - `old` must be valid to write the current context to.
/// - `new` must be valid and contain a context previously saved by this
///   function (or set up for initial thread entry).
#[inline(never)]
pub unsafe fn context_switch(old: *mut ThreadContext, new: *const ThreadContext) {
    asm!(
        "mov [rdi + 0], rsp",
        "mov rax, [rsp]",
        "mov [rdi + 8], rax",
        "mov [rdi + 16], rbx",
        "mov [rdi + 24], rbp",
        "mov [rdi + 32], r12",
        "mov [rdi + 40], r13",
        "mov [rdi + 48], r14",
        "mov [rdi + 56], r15",
        "mov rsp, [rsi + 0]",
        "mov rbx, [rsi + 16]",
        "mov rbp, [rsi + 24]",
        "mov r12, [rsi + 32]",
        "mov r13, [rsi + 40]",
        "mov r14, [rsi + 48]",
        "mov r15, [rsi + 56]",
        "sti",
        "ret",
        in("rdi") old,
        in("rsi") new,
        options(nostack)
    );
}

/// Restores context from `new` and jumps to that context's instruction pointer.
/// Does *not* save the current context (use when switching from interrupt context
/// after having already written the interrupted thread's state into its context).
/// Enables interrupts (sti) so the new thread receives timer/keyboard IRQs.
///
/// # Safety
/// - `new` must be valid and contain a context previously saved by
///   `context_switch` or set up for initial thread entry.
#[inline(never)]
pub unsafe fn context_switch_to(new: *const ThreadContext) {
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

#[cfg(test)]
mod tests {
    use super::ThreadContext;

    #[test_case]
    fn test_context_default() {
        let ctx = ThreadContext::default();
        assert_eq!(ctx.rsp, 0);
        assert_eq!(ctx.rip, 0);
        assert_eq!(ctx.rbx, 0);
        assert_eq!(ctx.rbp, 0);
        assert_eq!(ctx.r12, 0);
        assert_eq!(ctx.r13, 0);
        assert_eq!(ctx.r14, 0);
        assert_eq!(ctx.r15, 0);
    }

    #[test_case]
    fn test_context_clone() {
        let mut ctx1 = ThreadContext {
            rsp: 0x1000,
            rip: 0x2000,
            rbx: 0x3000,
            rbp: 0x4000,
            r12: 0x5000,
            r13: 0x6000,
            r14: 0x7000,
            r15: 0x8000,
        };
        let ctx2 = ctx1;
        ctx1.rsp = 0x9999;
        assert_eq!(ctx2.rsp, 0x1000);
        assert_eq!(ctx2.rip, 0x2000);
    }
}
