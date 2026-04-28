use alloc::boxed::Box;
use core::sync::atomic::{AtomicU64, Ordering};
use super::context::ThreadContext;
use super::stack::KernelStack;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ThreadId(pub u64);

impl ThreadId {
    fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        ThreadId(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

pub struct Thread {
    pub id: ThreadId,
    _stack: KernelStack,  // must be kept alive; context.rsp points into this allocation
    context: Box<ThreadContext>,
}

impl Thread {
    // Create a thread that starts at entry_point when first scheduled.
    // entry_point must not return.
    pub fn new(entry_point: fn()) -> Self {
        let mut stack = KernelStack::new();
        let mut context = Box::new(ThreadContext::default());
        stack.init_context(context.as_mut(), entry_point as usize);
        Thread {
            id: ThreadId::next(),
            _stack: stack,
            context,
        }
    }

    pub fn context_ptr(&mut self) -> *mut ThreadContext {
        self.context.as_mut() as *mut ThreadContext
    }
}

#[cfg(test)]
mod tests {
    use super::{Thread, ThreadId};

    fn dummy_entry() { loop {} }

    #[test_case]
    fn test_thread_creation() {
        let thread = Thread::new(dummy_entry);
        assert!(thread.id.0 > 0);
    }

    #[test_case]
    fn test_thread_ids_unique() {
        let t1 = Thread::new(dummy_entry);
        let t2 = Thread::new(dummy_entry);
        assert_ne!(t1.id, t2.id);
    }

    #[test_case]
    fn test_thread_context_ptr() {
        let mut thread = Thread::new(dummy_entry);
        assert!(!thread.context_ptr().is_null());
    }
}
