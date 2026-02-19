use core::{future::Future, pin::Pin};
use core::task::{Context, Poll};
use alloc::boxed::Box;
use core::sync::atomic::{AtomicU64, Ordering};


pub mod simple_executor;
pub mod keyboard;
pub mod executor;
pub mod shell;
pub mod sleep;


pub struct Task {
    id: TaskId,
    future: Pin<Box<dyn Future<Output = ()> + Send>>,
}

impl Task {
    pub fn new(future: impl Future<Output = ()> + Send + 'static) -> Task {
        Task {
            id: TaskId::new(),
            future: Box::pin(future),
        }
    }
}

impl Task {
    fn poll(&mut self, context: &mut Context) -> Poll<()> {
        self.future.as_mut().poll(context)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TaskId(u64);

impl TaskId {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        TaskId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
    
    pub(crate) fn as_u64(self) -> u64 {
        self.0
    }
}