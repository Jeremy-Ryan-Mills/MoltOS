// Saved CPU register state for a kernel thread.
//
// Offsets are used verbatim in inline asm — do not reorder fields without
// updating every asm block below and in interrupts.rs.

use core::arch::naked_asm;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ThreadContext {
    pub rsp:    u64,   // 0
    pub rip:    u64,   // 8
    pub rbx:    u64,   // 16
    pub rbp:    u64,   // 24
    pub r12:    u64,   // 32
    pub r13:    u64,   // 40
    pub r14:    u64,   // 48
    pub r15:    u64,   // 56
    // Caller-saved + rflags.  Only meaningful for preempted threads;
    // for threads that yielded cooperatively these fields are 0 and the
    // compiler's own call-site spill/reload handles the restoration.
    pub rax:    u64,   // 64
    pub rcx:    u64,   // 72
    pub rdx:    u64,   // 80
    pub rsi:    u64,   // 88
    pub rdi:    u64,   // 96
    pub r8:     u64,   // 104
    pub r9:     u64,   // 112
    pub r10:    u64,   // 120
    pub r11:    u64,   // 128
    pub rflags: u64,   // 136
}

impl Default for ThreadContext {
    fn default() -> Self {
        Self {
            rsp: 0, rip: 0,
            rbx: 0, rbp: 0, r12: 0, r13: 0, r14: 0, r15: 0,
            rax: 0, rcx: 0, rdx: 0, rsi: 0, rdi: 0,
            r8: 0,  r9: 0,  r10: 0, r11: 0,
            rflags: 0x202,  // IF=1, reserved bit 1
        }
    }
}

// Cooperative context switch: save current thread into `old`, restore `new`.
//
// Only callee-saved registers + rflags are saved for `old` — the Rust ABI
// guarantees caller-saved values are either dead or already spilled to `old`'s
// stack by the compiler.  The `new` thread is restored fully so that a
// previously preempted thread (with a full register save) resumes correctly.
//
// Naked so there is no function prologue that could shift rsp before the asm
// saves it.  `old` arrives in rdi and `new` in rsi per System V ABI.
#[unsafe(naked)]
pub unsafe extern "C" fn context_switch(old: *mut ThreadContext, new: *const ThreadContext) {
    naked_asm!(
        // Save rflags first (before cli) so old_ctx reflects the caller's
        // live state (IF=1); cli then locks out interrupts for the switch.
        "pushfq",
        "pop qword ptr [rdi + 136]",
        "cli",
        // --- save old ---
        "mov [rdi + 0],  rsp",
        "mov rax,        [rsp]",     // rip = return address sitting at [rsp]
        "mov [rdi + 8],  rax",
        "mov [rdi + 16], rbx",
        "mov [rdi + 24], rbp",
        "mov [rdi + 32], r12",
        "mov [rdi + 40], r13",
        "mov [rdi + 48], r14",
        "mov [rdi + 56], r15",
        // --- restore new (full) ---
        "mov rdi, rsi",              // rdi = new
        "mov rsp, [rdi + 0]",
        "mov rbx, [rdi + 16]",
        "mov rbp, [rdi + 24]",
        "mov r12, [rdi + 32]",
        "mov r13, [rdi + 40]",
        "mov r14, [rdi + 48]",
        "mov r15, [rdi + 56]",
        "mov rax, [rdi + 64]",
        "mov rcx, [rdi + 72]",
        "mov rdx, [rdi + 80]",
        "mov rsi, [rdi + 88]",
        "mov r8,  [rdi + 104]",
        "mov r9,  [rdi + 112]",
        "mov r10, [rdi + 120]",
        "mov r11, [rdi + 128]",
        "push qword ptr [rdi + 136]",
        "btr qword ptr [rsp], 9",    // clear IF so popfq doesn't enable interrupts mid-switch
        "popfq",
        "mov rdi, [rdi + 96]",
        "sti",
        "ret",
    );
}

// Restore `new` without saving the current context.
// Called from interrupt handlers after they have already written the
// interrupted thread's full state into its ThreadContext.
//
// Naked so there is no function prologue that could clobber rdi before the
// asm runs.  `new` arrives in rdi per System V ABI and is used directly.
#[unsafe(naked)]
pub unsafe extern "C" fn context_switch_to(new: *const ThreadContext) {
    naked_asm!(
        "mov rsp, [rdi + 0]",
        "mov rbx, [rdi + 16]",
        "mov rbp, [rdi + 24]",
        "mov r12, [rdi + 32]",
        "mov r13, [rdi + 40]",
        "mov r14, [rdi + 48]",
        "mov r15, [rdi + 56]",
        "mov rax, [rdi + 64]",
        "mov rcx, [rdi + 72]",
        "mov rdx, [rdi + 80]",
        "mov rsi, [rdi + 88]",
        "mov r8,  [rdi + 104]",
        "mov r9,  [rdi + 112]",
        "mov r10, [rdi + 120]",
        "mov r11, [rdi + 128]",
        "push qword ptr [rdi + 136]",
        "btr qword ptr [rsp], 9",    // clear IF so popfq doesn't enable interrupts mid-switch
        "popfq",
        "mov rdi, [rdi + 96]",
        "sti",
        "ret",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::arch::naked_asm;
    use core::mem::offset_of;
    use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use crate::thread::stack::KernelStack;

    // ---------------------------------------------------------------------------
    // Body functions for context-switch tests.
    //
    // The caller passes &old_ctx via new_ctx.rdi; context_switch loads it into
    // rdi as its last step before `ret`, so the body receives it as its first
    // argument (System V ABI).  Bodies call context_switch_to(old) to return.
    //
    // Avoiding global *_OLD_PTR statics eliminates any risk of a body reading a
    // stale zero if tests run in an unexpected order or if LLVM ICF merges two
    // similarly-structured functions.
    // ---------------------------------------------------------------------------

    static IF_BACK: AtomicBool = AtomicBool::new(false);

    unsafe extern "C" fn body_if_test(old: *const ThreadContext) {
        IF_BACK.store(true, Ordering::SeqCst);
        unsafe { context_switch_to(old); }
    }

    static RT_COUNTER: AtomicU64 = AtomicU64::new(0);

    unsafe extern "C" fn body_round_trip(old: *const ThreadContext) {
        RT_COUNTER.fetch_add(1, Ordering::SeqCst);
        unsafe { context_switch_to(old); }
    }

    // body_multi needs cooperative switching so it can save its own context for
    // re-entry; it uses globals because rdi is overwritten each time it resumes.
    static MULTI_COUNTER: AtomicU64 = AtomicU64::new(0);
    static MULTI_OLD_PTR: AtomicU64 = AtomicU64::new(0);
    static MULTI_NEW_PTR: AtomicU64 = AtomicU64::new(0);

    unsafe extern "C" fn body_multi() {
        loop {
            MULTI_COUNTER.fetch_add(1, Ordering::SeqCst);
            let old      = MULTI_OLD_PTR.load(Ordering::SeqCst) as *mut ThreadContext;
            let new_self = MULTI_NEW_PTR.load(Ordering::SeqCst) as *mut ThreadContext;
            unsafe { context_switch(&mut *new_self, &*old); }
        }
    }

    unsafe extern "C" fn body_canary(old: *const ThreadContext) {
        unsafe { context_switch_to(old); }
    }

    unsafe extern "C" fn body_rsp_check(old: *const ThreadContext) {
        unsafe { context_switch_to(old); }
    }

    static CALLEE_DONE: AtomicBool = AtomicBool::new(false);

    unsafe extern "C" fn body_callee_saved(old: *const ThreadContext) {
        CALLEE_DONE.store(true, Ordering::SeqCst);
        unsafe { context_switch_to(old); }
    }

    unsafe extern "C" fn body_saves_state(old: *const ThreadContext) {
        unsafe { context_switch_to(old); }
    }

    static REGS_RBX: AtomicU64 = AtomicU64::new(0);
    static REGS_R12: AtomicU64 = AtomicU64::new(0);
    static REGS_R13: AtomicU64 = AtomicU64::new(0);

    // Naked trampoline: captures rbx/r12/r13 before any Rust function prologue
    // can save/restore them, then tail-calls body_regs_inner.  rdi (= &old_ctx
    // passed via new_ctx.rdi) is not touched, so body_regs_inner receives it.
    #[unsafe(naked)]
    unsafe extern "C" fn body_regs_trampoline() {
        naked_asm!(
            "mov [{rbx_dst}], rbx",
            "mov [{r12_dst}], r12",
            "mov [{r13_dst}], r13",
            "jmp {inner}",
            rbx_dst = sym REGS_RBX,
            r12_dst = sym REGS_R12,
            r13_dst = sym REGS_R13,
            inner   = sym body_regs_inner,
        );
    }

    unsafe extern "C" fn body_regs_inner(old: *const ThreadContext) {
        unsafe { context_switch_to(old); }
    }

    // ---------------------------------------------------------------------------
    // Tests
    // ---------------------------------------------------------------------------

    #[test_case]
    fn test_context_default() {
        let ctx = ThreadContext::default();
        assert_eq!(ctx.rsp, 0);
        assert_eq!(ctx.rip, 0);
    }

    #[test_case]
    fn test_context_clone() {
        let ctx1 = ThreadContext { rsp: 0x1000, rip: 0x2000, rbx: 0x3000, rbp: 0x4000,
                                   r12: 0x5000, r13: 0x6000, r14: 0x7000, r15: 0x8000,
                                   ..ThreadContext::default() };
        let ctx2 = ctx1;
        assert_eq!(ctx2.rsp, 0x1000);
        assert_eq!(ctx2.rip, 0x2000);
    }

    // The context_switch* functions reference ThreadContext fields by hardcoded byte offsets.
    // These tests ensure the struct layout hasn't drifted from those offsets.
    #[test_case]
    fn test_context_field_offsets() {
        assert_eq!(offset_of!(ThreadContext, rsp),    0);
        assert_eq!(offset_of!(ThreadContext, rip),    8);
        assert_eq!(offset_of!(ThreadContext, rbx),    16);
        assert_eq!(offset_of!(ThreadContext, rbp),    24);
        assert_eq!(offset_of!(ThreadContext, r12),    32);
        assert_eq!(offset_of!(ThreadContext, r13),    40);
        assert_eq!(offset_of!(ThreadContext, r14),    48);
        assert_eq!(offset_of!(ThreadContext, r15),    56);
        assert_eq!(offset_of!(ThreadContext, rax),    64);
        assert_eq!(offset_of!(ThreadContext, rcx),    72);
        assert_eq!(offset_of!(ThreadContext, rdx),    80);
        assert_eq!(offset_of!(ThreadContext, rsi),    88);
        assert_eq!(offset_of!(ThreadContext, rdi),    96);
        assert_eq!(offset_of!(ThreadContext, r8),     104);
        assert_eq!(offset_of!(ThreadContext, r9),     112);
        assert_eq!(offset_of!(ThreadContext, r10),    120);
        assert_eq!(offset_of!(ThreadContext, r11),    128);
        assert_eq!(offset_of!(ThreadContext, rflags), 136);
    }

    // Default rflags must have IF=1 (bit 9) so that freshly-spawned threads
    // run with interrupts enabled and the scheduler can preempt them.
    #[test_case]
    fn test_context_default_rflags_if_set() {
        let ctx = ThreadContext::default();
        assert_ne!(ctx.rflags & 0x200, 0, "IF bit must be set in default rflags");
    }

    // All caller-saved fields introduced alongside the context-switch fix must
    // default to 0 so a new thread doesn't start with garbage register values.
    #[test_case]
    fn test_context_default_caller_saved_zero() {
        let ctx = ThreadContext::default();
        assert_eq!(ctx.rax, 0);
        assert_eq!(ctx.rcx, 0);
        assert_eq!(ctx.rdx, 0);
        assert_eq!(ctx.rsi, 0);
        assert_eq!(ctx.rdi, 0);
        assert_eq!(ctx.r8,  0);
        assert_eq!(ctx.r9,  0);
        assert_eq!(ctx.r10, 0);
        assert_eq!(ctx.r11, 0);
    }

    // After context_switch returns, the IF flag must be set so the caller
    // continues with interrupts enabled.  This exercises the btr+popfq+sti
    // sequence that replaced the bare popfq.
    #[test_case]
    fn test_interrupts_enabled_after_cooperative_switch() {
        let mut old_ctx = ThreadContext::default();
        let mut new_ctx = ThreadContext::default();
        let mut stack = KernelStack::new();
        stack.init_context(&mut new_ctx, body_if_test as *const () as usize);
        new_ctx.rdi = &old_ctx as *const ThreadContext as u64;

        unsafe { context_switch(&mut old_ctx, &new_ctx); }

        assert!(IF_BACK.load(Ordering::SeqCst));
        assert!(x86_64::instructions::interrupts::are_enabled(),
                "IF must be set after context_switch returns");
    }

    // A cooperative round-trip: the spawned thread executes, increments a counter,
    // then switches back.  The original caller verifies the counter was reached.
    #[test_case]
    fn test_cooperative_round_trip() {
        let mut old_ctx = ThreadContext::default();
        let mut new_ctx = ThreadContext::default();
        let mut stack = KernelStack::new();
        stack.init_context(&mut new_ctx, body_round_trip as *const () as usize);
        new_ctx.rdi = &old_ctx as *const ThreadContext as u64;

        assert_eq!(RT_COUNTER.load(Ordering::SeqCst), 0);
        unsafe { context_switch(&mut old_ctx, &new_ctx); }
        assert_eq!(RT_COUNTER.load(Ordering::SeqCst), 1);
    }

    // Switch to a thread and back N times to verify the mechanism isn't one-shot.
    // Each trip the thread increments a counter; the caller verifies the total.
    #[test_case]
    fn test_multiple_cooperative_switches() {
        const TRIPS: u64 = 5;
        let mut old_ctx = ThreadContext::default();
        let mut new_ctx = ThreadContext::default();
        let mut stack = KernelStack::new();
        stack.init_context(&mut new_ctx, body_multi as *const () as usize);
        MULTI_OLD_PTR.store(&mut old_ctx as *mut _ as u64, Ordering::SeqCst);
        MULTI_NEW_PTR.store(&mut new_ctx as *mut _ as u64, Ordering::SeqCst);

        for i in 1..=TRIPS {
            unsafe { context_switch(&mut old_ctx, &new_ctx); }
            assert_eq!(MULTI_COUNTER.load(Ordering::SeqCst), i);
        }
    }

    // Verify the stack canary on the switched-to thread's stack is intact
    // after a round-trip — a clobbered canary means the context switch wrote
    // outside the stack bounds or the stack top calculation is wrong.
    #[test_case]
    fn test_stack_canary_survives_switch() {
        let mut old_ctx = ThreadContext::default();
        let mut new_ctx = ThreadContext::default();
        let mut stack = KernelStack::new();
        stack.init_context(&mut new_ctx, body_canary as *const () as usize);
        new_ctx.rdi = &old_ctx as *const ThreadContext as u64;

        unsafe { context_switch(&mut old_ctx, &new_ctx); }

        assert!(stack.check_canary(), "stack canary overwritten during context switch");
    }

    // After a context switch the saved old context must reflect the actual rsp
    // at the call site — the resume stack pointer must point back into the
    // caller's frame, not into the new thread's stack.
    #[test_case]
    fn test_saved_rsp_is_within_caller_stack() {
        let mut old_ctx = ThreadContext::default();
        let mut new_ctx = ThreadContext::default();
        let mut stack = KernelStack::new();
        stack.init_context(&mut new_ctx, body_rsp_check as *const () as usize);
        new_ctx.rdi = &old_ctx as *const ThreadContext as u64;

        let local_addr = &old_ctx as *const _ as u64;
        unsafe { context_switch(&mut old_ctx, &new_ctx); }

        let diff = if old_ctx.rsp > local_addr {
            old_ctx.rsp - local_addr
        } else {
            local_addr - old_ctx.rsp
        };
        assert!(diff < 0x10000, "saved rsp 0x{:x} is far from caller frame 0x{:x}", old_ctx.rsp, local_addr);
    }

    // After a cooperative round-trip, local variables that the compiler keeps in
    // callee-saved registers across a function call must be unchanged.  black_box
    // forces the values to stay live, so the compiler must spill them into
    // callee-saved registers to survive the context_switch call.  If the save/
    // restore path is broken the assertions below will see garbage values.
    #[test_case]
    fn test_callee_saved_survive_round_trip() {
        let a = core::hint::black_box(0xAAAA_1111_BBBB_2222u64);
        let b = core::hint::black_box(0xCCCC_3333_DDDD_4444u64);
        let c = core::hint::black_box(0xEEEE_5555_FFFF_6666u64);

        let mut old_ctx = ThreadContext::default();
        let mut new_ctx = ThreadContext::default();
        let mut stack = KernelStack::new();
        stack.init_context(&mut new_ctx, body_callee_saved as *const () as usize);
        new_ctx.rdi = &old_ctx as *const ThreadContext as u64;

        unsafe { context_switch(&mut old_ctx, &new_ctx); }

        assert_eq!(core::hint::black_box(a), 0xAAAA_1111_BBBB_2222u64);
        assert_eq!(core::hint::black_box(b), 0xCCCC_3333_DDDD_4444u64);
        assert_eq!(core::hint::black_box(c), 0xEEEE_5555_FFFF_6666u64);
        assert!(CALLEE_DONE.load(Ordering::SeqCst));
    }

    // context_switch must save rsp, rip, and rflags into old_ctx so the
    // thread can be resumed later.  This verifies the save path ran and
    // produced coherent values.
    #[test_case]
    fn test_context_switch_saves_caller_state() {
        let mut old_ctx = ThreadContext::default();
        let mut new_ctx = ThreadContext::default();
        let mut stack = KernelStack::new();
        stack.init_context(&mut new_ctx, body_saves_state as *const () as usize);
        new_ctx.rdi = &old_ctx as *const ThreadContext as u64;

        unsafe { context_switch(&mut old_ctx, &new_ctx); }

        assert_ne!(old_ctx.rsp, 0, "rsp must be saved");
        assert_ne!(old_ctx.rip, 0, "rip must be saved");
        assert_ne!(old_ctx.rflags & 0x200, 0, "saved rflags must have IF set");
    }

    // context_switch must restore callee-saved register fields from new_ctx.
    // We pre-load new_ctx.rbx / r12 / r13 with sentinels; the naked trampoline
    // body_regs_trampoline captures those live register values before any Rust
    // function prologue can save and restore them.  &old_ctx is passed via
    // new_ctx.rdi; the trampoline leaves rdi untouched so body_regs_inner gets it.
    #[test_case]
    fn test_new_ctx_callee_regs_restored() {
        let mut old_ctx = ThreadContext::default();
        let mut new_ctx = ThreadContext::default();
        let mut stack = KernelStack::new();
        // init_context resets rbx/r12/r13; set sentinels after.
        stack.init_context(&mut new_ctx, body_regs_trampoline as *const () as usize);
        new_ctx.rbx = 0xCAFE_BABE_0001;
        new_ctx.r12 = 0xCAFE_BABE_0002;
        new_ctx.r13 = 0xCAFE_BABE_0003;
        new_ctx.rdi = &old_ctx as *const ThreadContext as u64;

        unsafe { context_switch(&mut old_ctx, &new_ctx); }

        assert_eq!(REGS_RBX.load(Ordering::SeqCst), 0xCAFE_BABE_0001, "rbx not restored");
        assert_eq!(REGS_R12.load(Ordering::SeqCst), 0xCAFE_BABE_0002, "r12 not restored");
        assert_eq!(REGS_R13.load(Ordering::SeqCst), 0xCAFE_BABE_0003, "r13 not restored");
    }
}
