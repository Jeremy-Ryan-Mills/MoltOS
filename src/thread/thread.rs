//! Kernel thread descriptor.

use alloc::boxed::Box;
use core::sync::atomic::{AtomicU64, Ordering};

use super::context::ThreadContext;
use super::stack::KernelStack;

/// Opaque thread ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadId(pub u64);

impl ThreadId {
    fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        ThreadId(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// A kernel thread: owns a stack and saved context.
pub struct Thread {
    pub id: ThreadId,
    stack: KernelStack,
    context: Box<ThreadContext>,
}

impl Thread {
    /// Creates a new thread that will start at `entry_point` when first scheduled.
    ///
    /// `entry_point` must be a `fn()` that does not return (or loops forever).
    pub fn new(entry_point: fn()) -> Self {
        let mut stack = KernelStack::new();
        let mut context = Box::new(ThreadContext::default());
        stack.init_context(context.as_mut(), entry_point as usize);
        Thread {
            id: ThreadId::next(),
            stack,
            context,
        }
    }

    /// Returns a pointer to the context for use in context_switch.
    pub fn context_ptr(&mut self) -> *mut ThreadContext {
        self.context.as_mut() as *mut ThreadContext
    }
}

#[cfg(test)]
mod tests {
    use super::{Thread, ThreadId};

    fn dummy_entry() {
        loop {}
    }

    #[test_case]
    fn test_thread_creation() {
        let thread = Thread::new(dummy_entry);
        assert!(thread.id.0 > 0);
    }

    #[test_case]
    fn test_thread_ids_unique() {
        let thread1 = Thread::new(dummy_entry);
        let thread2 = Thread::new(dummy_entry);
        assert_ne!(thread1.id, thread2.id);
    }

    #[test_case]
    fn test_thread_context_ptr() {
        let mut thread = Thread::new(dummy_entry);
        let ptr = thread.context_ptr();
        assert!(!ptr.is_null());
    }
}
