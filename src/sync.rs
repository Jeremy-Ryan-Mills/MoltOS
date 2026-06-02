// IrqMutex: disables interrupts on lock to prevent deadlock when the timer handler
// also acquires the same lock (e.g. SCHEDULER).
//
// KMutex: sleeping mutex — blocks the thread via the scheduler instead of spinning.
// Not safe to call from interrupt handlers or before enter_scheduler().

use core::cell::UnsafeCell;
use core::mem::ManuallyDrop;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};
use alloc::collections::VecDeque;
use spin::Mutex;

// ---------------------------------------------------------------------------
// IrqMutex
// ---------------------------------------------------------------------------

pub struct IrqMutex<T>(Mutex<T>);

pub struct IrqMutexGuard<'a, T> {
    inner:       ManuallyDrop<spin::MutexGuard<'a, T>>,
    was_enabled: bool,
}

impl<T> IrqMutex<T> {
    pub const fn new(data: T) -> Self {
        Self(Mutex::new(data))
    }

    /// Lock and disable interrupts. The guard restores the interrupt state on drop.
    pub fn lock(&self) -> IrqMutexGuard<'_, T> {
        let was_enabled = x86_64::instructions::interrupts::are_enabled();
        x86_64::instructions::interrupts::disable();
        IrqMutexGuard { inner: ManuallyDrop::new(self.0.lock()), was_enabled }
    }
}

// Safe: IrqMutex provides mutual exclusion.
unsafe impl<T: Send> Sync for IrqMutex<T> {}
unsafe impl<T: Send> Send for IrqMutex<T> {}

impl<T> Deref for IrqMutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T { &**self.inner }
}

impl<T> DerefMut for IrqMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T { &mut **self.inner }
}

impl<'a, T> IrqMutexGuard<'a, T> {
    /// Release the spinlock without restoring the interrupt flag.
    ///
    /// Use this before a cooperative context switch: the spinlock is released
    /// so other cores (or interrupt handlers after iret) can acquire it, but
    /// interrupts stay disabled until context_switch re-enables them via `sti`.
    /// This closes the window where a timer could fire between "mark self
    /// blocked" and "the actual context switch", which would corrupt scheduler
    /// state (scheduler.current would point to the wrong thread).
    pub fn release_without_irq_restore(mut self) {
        unsafe { ManuallyDrop::drop(&mut self.inner); }
        core::mem::forget(self);
        // Interrupts remain in whatever state IrqMutex::lock left them (disabled).
    }
}

impl<T> Drop for IrqMutexGuard<'_, T> {
    fn drop(&mut self) {
        // Release spinlock first, then restore interrupts — never hold the lock
        // while re-enabling interrupts (a timer could fire and try to re-acquire).
        unsafe { ManuallyDrop::drop(&mut self.inner); }
        if self.was_enabled {
            x86_64::instructions::interrupts::enable();
        }
    }
}

// ---------------------------------------------------------------------------
// KMutex
// ---------------------------------------------------------------------------

pub struct KMutex<T> {
    locked:  AtomicBool,
    waiters: Mutex<VecDeque<crate::thread::ThreadId>>,
    data:    UnsafeCell<T>,
}

pub struct KMutexGuard<'a, T> {
    mutex: &'a KMutex<T>,
}

unsafe impl<T: Send> Sync for KMutex<T> {}
unsafe impl<T: Send> Send for KMutex<T> {}

impl<T> KMutex<T> {
    pub const fn new(data: T) -> Self {
        Self {
            locked:  AtomicBool::new(false),
            waiters: Mutex::new(VecDeque::new()),
            data:    UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> KMutexGuard<'_, T> {
        loop {
            // Fast path: uncontended acquire.
            if self.locked.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
                return KMutexGuard { mutex: self };
            }

            // Slow path: take the scheduler lock (IrqMutex → disables interrupts)
            // and recheck atomically.  Holding the scheduler lock prevents the
            // current mutex holder's drop() from calling unblock_thread between
            // our "add to waiters" and "mark blocked" steps — that would cause a
            // lost wakeup on single-core systems.
            let mut sched = crate::thread::SCHEDULER.lock();

            if self.locked.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
                return KMutexGuard { mutex: self };
                // sched dropped: SCHEDULER released, interrupts restored
            }

            match sched.current_thread_id() {
                Some(id) => {
                    self.waiters.lock().push_back(id);
                    let switch = sched.block_current_and_prepare_switch(crate::uptime_ticks());
                    // Release the scheduler spinlock but keep interrupts disabled.
                    // A timer firing here would see us as blocked and switch away;
                    // when we resume we would do context_switch again with stale
                    // pointers, corrupting scheduler.current.  Keeping interrupts
                    // off until context_switch (which does sti) closes this window.
                    sched.release_without_irq_restore();

                    match switch {
                        Some((from, to)) => unsafe { crate::thread::context::context_switch(from, to); },
                        None => {
                            crate::serial_println!("KMutex: DEADLOCK — all threads blocked");
                            loop { x86_64::instructions::interrupts::enable_and_hlt(); }
                        }
                    }
                    // After context_switch returns, interrupts are enabled (sti in asm).
                }
                None => {
                    // Not in a scheduled thread (bootstrap or interrupt context).
                    // sched guard dropped here: SCHEDULER released, interrupts restored.
                }
            }

            core::hint::spin_loop();
        }
    }
}

impl<T> Deref for KMutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T { unsafe { &*self.mutex.data.get() } }
}

impl<T> DerefMut for KMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T { unsafe { &mut *self.mutex.data.get() } }
}

impl<T> Drop for KMutexGuard<'_, T> {
    fn drop(&mut self) {
        // Release the data lock first so the woken thread can acquire it immediately.
        self.mutex.locked.store(false, Ordering::Release);

        // Wake one waiter.  IrqMutex.lock() disables interrupts, so the timer
        // handler can't deadlock on SCHEDULER while we hold it here.
        if let Some(id) = self.mutex.waiters.lock().pop_front() {
            crate::thread::SCHEDULER.lock().unblock_thread(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::KMutex;

    #[test_case]
    fn test_kmutex_lock_unlock() {
        let m = KMutex::new(0u32);
        { let mut g = m.lock(); *g = 42; }
        assert_eq!(*m.lock(), 42);
    }

    #[test_case]
    fn test_kmutex_multiple_cycles() {
        let m = KMutex::new(0u32);
        for i in 0..10 { *m.lock() = i; }
        assert_eq!(*m.lock(), 9);
    }

    #[test_case]
    fn test_kmutex_nested_data() {
        let m = KMutex::new([0u8; 8]);
        { m.lock()[3] = 0xff; }
        assert_eq!(m.lock()[3], 0xff);
    }
}
