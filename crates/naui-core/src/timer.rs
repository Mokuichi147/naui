//! UI スレッドで、時間を置いて (または一定の間隔で) 呼ぶ。
//!
//! 待つのはバックエンドのイベントループ ([`MainThread::post_after`]) で、
//! コールバックは UI スレッドで呼ばれる。登録簿はチャネルやタスクと同じ
//! ものを使い、スレッドをまたぐのは番号だけにする。
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
    /// 繰り返す間隔。1 回きりなら `None`。
    repeat: Option<Duration>,
    active: Cell<bool>,
    dispatch: Arc<AppDispatch>,
}

impl TimerLocal {
    /// 次の呼び出しを予約する。積めなければ止める。
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

/// [`Tasks::after`](crate::Tasks::after) / [`Tasks::every`](crate::Tasks::every)
/// が返すタイマーの取っ手。
///
/// **落としてもタイマーは止まらない** ([`Task`](crate::Task) と同じ)。止めたい
/// ときは [`cancel`](Timer::cancel) を呼ぶ。clone しても同じタイマーを指す。
#[derive(Clone)]
pub struct Timer {
    local: Weak<TimerLocal>,
}

impl Timer {
    /// 止める。まだ呼ばれていないものは呼ばれない。コールバックの中から
    /// 呼んでもよい。
    pub fn cancel(&self) {
        if let Some(local) = self.local.upgrade() {
            local.stop();
        }
    }

    /// まだ呼ばれる予定があるか。1 回きりのものは呼ばれた時点で `false`。
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

/// [`Tasks::sleep`](crate::Tasks::sleep) が返す future。
///
/// 落とすと待つのをやめる (タイマーも止める)。
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
