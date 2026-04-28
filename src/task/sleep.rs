// Timer-based async sleep. Sleep completes after a given number of PIT ticks.
// The executor must call wake_sleepers() each loop iteration.

use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use spin::Mutex;

static SLEEPERS: Mutex<Vec<(u64, Waker)>> = Mutex::new(Vec::new());

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

#[must_use = "futures do nothing unless awaited"]
pub struct Sleep {
    target_tick: u64,
    registered: bool,
}

impl Sleep {
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
        if crate::uptime_ticks() >= self.target_tick {
            return Poll::Ready(());
        }
        if !self.registered {
            SLEEPERS.lock().push((self.target_tick, cx.waker().clone()));
            self.registered = true;
        }
        Poll::Pending
    }
}
