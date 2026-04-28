/***
 * src/sync.rs
 *
 * Kernel synchronization primitives.
 *
 * KMutex<T>: a mutex that yields the current thread to the scheduler instead
 * of spinning. Use this for any shared state accessed from kernel threads.
 * spin::Mutex is still appropriate for very short critical sections (e.g. a
 * single push to a queue) and for use from interrupt handlers.
 *
 * Not safe to call KMutex::lock() from:
 *   - interrupt handlers (you'll block the whole CPU)
 *   - bootstrap (kernel_main) context before enter_scheduler()
 */

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};
use alloc::collections::VecDeque;
use spin::Mutex;

pub struct KMutex<T> {
    locked: AtomicBool,
    waiters: Mutex<VecDeque<crate::thread::ThreadId>>,
    data: UnsafeCell<T>,
}

pub struct KMutexGuard<'a, T> {
    mutex: &'a KMutex<T>,
}

// Safe to share across threads because lock() enforces mutual exclusion.
unsafe impl<T: Send> Sync for KMutex<T> {}
unsafe impl<T: Send> Send for KMutex<T> {}

impl<T> KMutex<T> {
    pub const fn new(data: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            waiters: Mutex::new(VecDeque::new()),
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> KMutexGuard<'_, T> {
        loop {
            // Fast path: mutex is free — grab it and return immediately.
            if self.locked.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
                return KMutexGuard { mutex: self };
            }

            // Slow path: mutex is held. We need to add ourselves to the waiter list
            // and block atomically.
            //
            // Disable interrupts so the timer can't switch us out between "add to
            // waiters" and "block" — that would cause a lost wakeup where unlock
            // pops our ID from waiters and calls unblock_thread before we've blocked,
            // leaving us sleeping forever.
            x86_64::instructions::interrupts::disable();

            // Recheck: the mutex may have been released while we were on our way here.
            if self.locked.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
                x86_64::instructions::interrupts::enable();
                return KMutexGuard { mutex: self };
            }

            let id = crate::thread::SCHEDULER.lock().current_thread_id();
            match id {
                Some(id) => {
                    self.waiters.lock().push_back(id);
                    // Block and yield. Interrupts are disabled here; they'll be
                    // re-enabled inside context_switch (sti).
                    unsafe { crate::thread::block_and_yield(); }
                    // We're back — interrupts are now enabled. Loop and try to acquire.
                }
                None => {
                    // Not in a scheduled thread (bootstrap or interrupt context).
                    // Fall back to spinning rather than blocking.
                    x86_64::instructions::interrupts::enable();
                    core::hint::spin_loop();
                }
            }
        }
    }
}

impl<T> Deref for KMutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<T> DerefMut for KMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<T> Drop for KMutexGuard<'_, T> {
    fn drop(&mut self) {
        // Release the lock first so another thread can acquire it immediately after wakeup.
        self.mutex.locked.store(false, Ordering::Release);

        // Wake one waiting thread. The woken thread will retry lock() and either
        // succeed or re-queue itself if someone else grabbed the lock first.
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
        {
            let mut g = m.lock();
            *g = 42;
        }
        assert_eq!(*m.lock(), 42);
    }

    #[test_case]
    fn test_kmutex_multiple_cycles() {
        let m = KMutex::new(0u32);
        for i in 0..10 {
            *m.lock() = i;
        }
        assert_eq!(*m.lock(), 9);
    }

    #[test_case]
    fn test_kmutex_nested_data() {
        let m = KMutex::new([0u8; 8]);
        {
            let mut g = m.lock();
            g[3] = 0xff;
        }
        assert_eq!(m.lock()[3], 0xff);
    }
}
