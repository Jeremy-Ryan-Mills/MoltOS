//! Timer-based async sleep.
//!
//! `Sleep` is a future that completes after a given number of PIT timer ticks.
//! The executor must call `wake_sleepers()` at the start of each run loop so
//! that sleepers are woken when their target tick is reached.

use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use spin::Mutex;

static SLEEPERS: Mutex<Vec<(u64, Waker)>> = Mutex::new(Vec::new());

/// Must be called at the start of each executor run loop so sleepers
/// are woken when `uptime_ticks()` reaches their target.
pub fn wake_sleepers() {
    let now = crate::uptime_ticks();
    let mut sleepers = SLEEPERS.lock();
    let mut i = 0;
    while i < sleepers.len() {
        if sleepers[i].0 <= now {
            let (_, waker) = sleepers.remove(i);
            waker.wake_by_ref();
        } else {
            i += 1;
        }
    }
}

/// A future that completes after `ticks` PIT timer ticks from now.
#[must_use = "futures do nothing unless awaited"]
pub struct Sleep {
    target_tick: u64,
    registered: bool,
}

impl Sleep {
    /// Creates a Sleep that completes after `ticks` timer ticks from now.
    pub fn new(ticks: u64) -> Self {
        Self {
            target_tick: crate::uptime_ticks().saturating_add(ticks),
            registered: false,
        }
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let now = crate::uptime_ticks();
        if now >= self.target_tick {
            return Poll::Ready(());
        }
        if !self.registered {
            SLEEPERS.lock().push((self.target_tick, cx.waker().clone()));
            self.registered = true;
        }
        Poll::Pending
    }
}
