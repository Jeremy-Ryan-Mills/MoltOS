use conquer_once::spin::OnceCell;
use crossbeam_queue::ArrayQueue;
use futures_util::{stream::Stream, task::AtomicWaker};
use core::{pin::Pin, task::{Context, Poll}};

static SCANCODE_QUEUE: OnceCell<ArrayQueue<u8>> = OnceCell::uninit();
static WAKER: AtomicWaker = AtomicWaker::new();

// Call once from kernel_main after heap init so keyboard IRQs can enqueue
// scancodes before the shell task starts.
pub fn init_scancode_queue() {
    let _ = SCANCODE_QUEUE.try_init_once(|| ArrayQueue::new(100));
}

pub(crate) fn add_scancode(scancode: u8) {
    if let Ok(queue) = SCANCODE_QUEUE.try_get() {
        if queue.push(scancode).is_ok() {
            WAKER.wake();
        }
        // Drop silently if queue is full
    }
    // Drop silently if queue not yet initialized (shell hasn't started)
}

pub struct ScancodeStream {
    _private: (),
}

impl ScancodeStream {
    pub fn new() -> Self {
        if SCANCODE_QUEUE.try_get().is_err() {
            SCANCODE_QUEUE.try_init_once(|| ArrayQueue::new(100))
                .expect("ScancodeStream::new failed to init queue");
        }
        ScancodeStream { _private: () }
    }
}

impl Stream for ScancodeStream {
    type Item = u8;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<u8>> {
        let queue = SCANCODE_QUEUE.try_get().expect("scancode queue not initialized");

        // Register waker before checking queue to avoid a race where a scancode
        // arrives between the empty-check and returning Pending.
        WAKER.register(cx.waker());

        match queue.pop() {
            Some(scancode) => Poll::Ready(Some(scancode)),
            None => Poll::Pending,
        }
    }
}
