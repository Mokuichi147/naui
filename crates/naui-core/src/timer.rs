//! Calls something on the UI thread after a delay (or at a fixed interval).
//!
//! The waiting is done by the backend's event loop ([`MainThread::post_after`]),
//! and callbacks are called on the UI thread. It uses the same registry as
//! channels and tasks, and only ids cross threads.
//!
//! [`MainThread::post_after`]: crate::MainThread::post_after

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use crate::main_thread::{deregister, next_id, pump_by_id, register, AppDispatch, LocalEntry};
use crate::Slot;

struct TimerLocal {
    id: u64,
    callback: RefCell<Slot<dyn FnMut()>>,
    /// The repeat interval. `None` for a one-shot timer.
    repeat: Option<Duration>,
    active: Cell<bool>,
    dispatch: Arc<AppDispatch>,
}

impl TimerLocal {
    /// Schedules the next call. Stops if it cannot be queued.
    fn schedule(&self, delay: Duration) {
        let id = self.id;
        if !self
            .dispatch
            .post_after(delay, Box::new(move || pump_by_id(id)))
        {
            self.stop();
        }
    }

    fn stop(&self) {
        if self.active.replace(false) {
            // 呼び出し中に止められても、コールバックはここでは落とさない
            // (取り出してあるので、戻ってから捨てられる)。
            self.callback.borrow_mut().take();
            deregister(self.id);
        }
    }
}

impl LocalEntry for TimerLocal {
    fn pump(&self) {
        if !self.active.get() {
            return;
        }
        let Some(mut callback) = self.callback.borrow_mut().take() else {
            return;
        };
        // 1 回きりのものは、呼ぶ前に終わったことにしておく (中から
        // `is_active` を見たときに `false` になるように)。
        if self.repeat.is_none() {
            self.stop();
        }
        // panic してもイベントループは巻き戻さない (タスクと同じ扱い)。
        let _ = catch_unwind(AssertUnwindSafe(&mut callback));
        if !self.active.get() {
            return;
        }
        *self.callback.borrow_mut() = Some(callback);
        if let Some(interval) = self.repeat {
            self.schedule(interval);
        }
    }
}

pub(crate) fn start(
    dispatch: &Arc<AppDispatch>,
    delay: Duration,
    repeat: Option<Duration>,
    callback: Box<dyn FnMut()>,
) -> Timer {
    let id = next_id();
    let local = Rc::new(TimerLocal {
        id,
        callback: RefCell::new(Some(callback)),
        repeat,
        active: Cell::new(true),
        dispatch: Arc::clone(dispatch),
    });
    register(id, local.clone());
    local.schedule(delay);
    Timer {
        local: Rc::downgrade(&local),
    }
}

/// The timer handle returned by [`Tasks::after`](crate::Tasks::after) /
/// [`Tasks::every`](crate::Tasks::every).
///
/// **Dropping it does not stop the timer** (the same as [`Task`](crate::Task)).
/// Call [`cancel`](Timer::cancel) to stop it. Clones point to the same timer.
#[derive(Clone)]
pub struct Timer {
    local: Weak<TimerLocal>,
}

impl Timer {
    /// Stops it. Calls that have not happened yet will not happen. May be called from
    /// inside the callback.
    pub fn cancel(&self) {
        if let Some(local) = self.local.upgrade() {
            local.stop();
        }
    }

    /// Whether more calls are still scheduled. A one-shot timer becomes `false` once it
    /// has been called.
    pub fn is_active(&self) -> bool {
        self.local.upgrade().is_some_and(|local| local.active.get())
    }
}

impl std::fmt::Debug for Timer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Timer")
            .field("active", &self.is_active())
            .finish()
    }
}

#[derive(Default)]
struct SleepState {
    done: Cell<bool>,
    waker: RefCell<Option<Waker>>,
}

/// The future returned by [`Tasks::sleep`](crate::Tasks::sleep).
///
/// Dropping it stops waiting (and stops the timer).
pub struct Sleep {
    delay: Duration,
    dispatch: Arc<AppDispatch>,
    state: Rc<SleepState>,
    timer: Option<Timer>,
}

impl Sleep {
    pub(crate) fn new(dispatch: &Arc<AppDispatch>, delay: Duration) -> Self {
        Self {
            delay,
            dispatch: Arc::clone(dispatch),
            state: Rc::default(),
            timer: None,
        }
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.state.done.get() {
            return Poll::Ready(());
        }
        *self.state.waker.borrow_mut() = Some(cx.waker().clone());
        if self.timer.is_none() {
            let state = Rc::clone(&self.state);
            let timer = start(
                &self.dispatch,
                self.delay,
                None,
                Box::new(move || {
                    state.done.set(true);
                    if let Some(waker) = state.waker.borrow_mut().take() {
                        waker.wake();
                    }
                }),
            );
            self.timer = Some(timer);
        }
        Poll::Pending
    }
}

impl Drop for Sleep {
    fn drop(&mut self) {
        if let Some(timer) = self.timer.take() {
            timer.cancel();
        }
    }
}
