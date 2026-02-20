//! Syscall interface.
//!
//! Invoke the kernel via `int 0x80`. Syscall number in `rax`, up to 3 args in
//! `rdi`, `rsi`, `rdx`; return value in `rax`.

use core::sync::atomic::{AtomicU64, Ordering};
use crate::print;
use crate::uptime_ticks;

/// Return value from the last syscall (written by handler, read by invoke helpers).
/// Used because the IDT expects a void handler so we can't leave the result in rax.
static SYSCALL_RETURN: AtomicU64 = AtomicU64::new(0);

/// Syscall vector (software interrupt).
pub const SYSCALL_VECTOR: u8 = 0x80;

/// Syscall numbers.
#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Syscall {
    /// Return uptime in timer ticks.
    Uptime = 0,
    /// Print one character. arg1 = character (u8 as u64).
    PutChar = 1,
    /// Exit (for future use). arg1 = exit code.
    Exit = 2,
}

impl Syscall {
    pub const fn as_u64(self) -> u64 {
        self as u64
    }
}

/// Dispatches the syscall and returns the result (for the caller's rax).
/// Called from the IDT handler; reads rax, rdi, rsi, rdx at entry via asm.
#[allow(unused_variables, path_statements)]
fn dispatch(number: u64, arg1: u64, arg2: u64, arg3: u64) -> u64 {
    match number {
        0 => {
            // SYS_UPTIME
            uptime_ticks()
        }
        1 => {
            // SYS_PUTCHAR
            let c = (arg1 & 0xff) as u8;
            if c == b'\n' {
                crate::println!();
            } else {
                print!("{}", c as char);
            }
            0
        }
        2 => {
            // SYS_EXIT - no-op for now (no user processes)
            0
        }
        _ => u64::MAX, // unknown syscall
    }
}

/// Syscall handler. Invoked by CPU on `int 0x80`.
/// Caller has: rax = syscall number, rdi = arg1, rsi = arg2, rdx = arg3.
/// We write the return value to SYSCALL_RETURN so the void handler can return
/// and the invoke helpers can read it (IDT expects fn(InterruptStackFrame)).
pub extern "x86-interrupt" fn syscall_handler(
    _stack_frame: x86_64::structures::idt::InterruptStackFrame,
) {
    let number: u64;
    let arg1: u64;
    let arg2: u64;
    let arg3: u64;
    unsafe {
        core::arch::asm!(
            "mov {}, rax",
            "mov {}, rdi",
            "mov {}, rsi",
            "mov {}, rdx",
            out(reg) number,
            out(reg) arg1,
            out(reg) arg2,
            out(reg) arg3,
            options(nostack, preserves_flags)
        );
    }
    let result = dispatch(number, arg1, arg2, arg3);
    SYSCALL_RETURN.store(result, Ordering::SeqCst);
}

/// Invoke a syscall with no arguments.
#[inline(always)]
pub fn syscall0(n: u64) -> u64 {
    unsafe {
        core::arch::asm!(
            "int 0x80",
            in("rax") n,
            options(nostack)
        );
    }
    SYSCALL_RETURN.load(Ordering::SeqCst)
}

/// Invoke a syscall with one argument.
#[inline(always)]
pub fn syscall1(n: u64, a1: u64) -> u64 {
    unsafe {
        core::arch::asm!(
            "int 0x80",
            in("rax") n,
            in("rdi") a1,
            options(nostack)
        );
    }
    SYSCALL_RETURN.load(Ordering::SeqCst)
}

/// Invoke a syscall with two arguments.
#[inline(always)]
pub fn syscall2(n: u64, a1: u64, a2: u64) -> u64 {
    unsafe {
        core::arch::asm!(
            "int 0x80",
            in("rax") n,
            in("rdi") a1,
            in("rsi") a2,
            options(nostack)
        );
    }
    SYSCALL_RETURN.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::{Syscall, syscall0, syscall1, syscall2};

    #[test_case]
    fn test_syscall_numbers() {
        crate::serial_print!("syscall::tests::test_syscall_numbers...\t");
        assert_eq!(Syscall::Uptime.as_u64(), 0);
        assert_eq!(Syscall::PutChar.as_u64(), 1);
        assert_eq!(Syscall::Exit.as_u64(), 2);
        crate::serial_println!("[ok]");
    }

    #[test_case]
    fn test_syscall_uptime() {
        crate::serial_print!("syscall::tests::test_syscall_uptime...\t");
        if !x86_64::instructions::interrupts::are_enabled() {
            x86_64::instructions::interrupts::enable();
        }
        let direct = crate::uptime_ticks();
        let via_syscall = syscall0(Syscall::Uptime.as_u64());
        assert_eq!(via_syscall, direct, "syscall(SYS_UPTIME) should match uptime_ticks()");
        crate::serial_println!("[ok]");
    }

    #[test_case]
    fn test_syscall_unknown() {
        crate::serial_print!("syscall::tests::test_syscall_unknown...\t");
        let ret = syscall0(999);
        assert_eq!(ret, u64::MAX, "unknown syscall should return u64::MAX");
        crate::serial_println!("[ok]");
    }

    #[test_case]
    fn test_syscall_putchar() {
        crate::serial_print!("syscall::tests::test_syscall_putchar...\t");
        let ret = syscall1(Syscall::PutChar.as_u64(), b'X' as u64);
        assert_eq!(ret, 0);
        crate::serial_println!("[ok]");
    }

    #[test_case]
    fn test_syscall_exit_noop() {
        crate::serial_print!("syscall::tests::test_syscall_exit_noop...\t");
        let ret = syscall2(Syscall::Exit.as_u64(), 0, 0);
        assert_eq!(ret, 0, "SYS_EXIT is a no-op and returns 0");
        crate::serial_println!("[ok]");
    }
}
