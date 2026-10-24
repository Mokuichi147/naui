//! A small executor that runs futures on the UI thread.
//!
//! It uses `std::task::Wake`, so there is no hand-written `RawWakerVTable` and the
//! crate's `forbid(unsafe_code)` is preserved.
//!
//! A `Waker` must be `Send + Sync`, while the future itself should stay `!Send` (so
//! it can hold widgets). So "the waking side" ([`TaskWake`]) and "the actual
//! object" ([`TaskLocal`]) are separate types, and only an id crosses threads.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use crate::main_thread::{deregister, next_id, pump_by_id, register, AppDispatch, LocalEntry};

/// The waking side. It is the contents of a `Waker`, so it must be `Send + Sync`.
struct TaskWake {
    id: u64,
    /// Whether the next poll is already scheduled. Collapses multiple wakes into one.
    scheduled: AtomicBool,
    /// Whether it is still running. Even if an old `Waker` wakes it after completion or
    /// cancellation, nothing is posted while this is `false`.
    active: AtomicBool,
    dispatch: Arc<AppDispatch>,
}

impl TaskWake {
    /// Schedules the next poll. `true` if queued.
    fn schedule(&self) -> bool {
        if !self.active.load(Ordering::Acquire) {
            return false;
        }
        if self.scheduled.swap(true, Ordering::AcqRel) {
            return true;
        }
        let id = self.id;
        if self.dispatch.post(Box::new(move || pump_by_id(id))) {
            true
        } else {
            self.scheduled.store(false, Ordering::Release);
            false
        }
    }
}

impl Wake for TaskWake {
    fn wake(self: Arc<Self>) {
        self.schedule();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.schedule();
    }
}

/// The actual object. UI-thread only; held by the registry.
struct TaskLocal {
    id: u64,
    future: RefCell<Option<Pin<Box<dyn Future<Output = ()>>>>>,
    cancelled: Cell<bool>,
    finished: Cell<bool>,
    /// Whether it is in the middle of a poll (to tell when it is cancelled from inside
    /// the poll).
    polling: Cell<bool>,
    wake: Arc<TaskWake>,
    waker: RefCell<Option<Waker>>,
}

impl TaskLocal {
    /// Cleanup at the end. Drops the future, stops posts from old `Waker`s, and removes
    /// it from the registry.
    fn finish(&self) {
        self.finished.set(true);
        self.wake.active.store(false, Ordering::Release);
        let future = self.future.borrow_mut().take();
        drop(future);
        let waker = self.waker.borrow_mut().take();
        drop(waker);
        deregister(self.id);
    }

    fn cancel(&self) {
        if self.finished.get() {
            return;
        }
        self.cancelled.set(true);
        if !self.polling.get() {
            self.finish();
        }
        // poll の最中なら、戻ったところで `pump` が cancelled を見て片付ける。
    }
}

impl LocalEntry for TaskLocal {
    fn pump(&self) {
        // **poll する前に予約を解く。** future が poll の中から自分を起こしたとき、
        // ここが `true` のままだと「予約済み」とみなされて wake が消えてしまう。
        self.wake.scheduled.store(false, Ordering::Release);

        if self.finished.get() || self.cancelled.get() {
            return;
        }
        if self.polling.replace(true) {
            return;
        }
        let taken = self.future.borrow_mut().take();
        let waker = self.waker.borrow().clone();
        let (Some(mut future), Some(waker)) = (taken, waker) else {
            self.polling.set(false);
            return;
        };

        let mut cx = Context::from_waker(&waker);
        // panic はここで止める (どのバックエンドでも C / WinRT の境界を越えて
        // 巻き戻せないため)。内容は既定の panic hook が stderr へ出す。
        let polled = catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(&mut cx)));
        self.polling.set(false);

        if matches!(polled, Ok(Poll::Pending)) && !self.cancelled.get() {
            *self.future.borrow_mut() = Some(future);
            return;
        }
        drop(future);
        self.finish();
    }
}

pub(crate) fn spawn<F>(dispatch: &Arc<AppDispatch>, future: F) -> Task
where
    F: Future<Output = ()> + 'static,
{
    let id = next_id();
    let wake = Arc::new(TaskWake {
        id,
        scheduled: AtomicBool::new(false),
        active: AtomicBool::new(true),
        dispatch: Arc::clone(dispatch),
    });
    let waker = Waker::from(Arc::clone(&wake));
    let local = Rc::new(TaskLocal {
        id,
        future: RefCell::new(Some(Box::pin(future))),
        cancelled: Cell::new(false),
        finished: Cell::new(false),
        polling: Cell::new(false),
        wake: Arc::clone(&wake),
        waker: RefCell::new(Some(waker)),
    });
    let handle = Rc::downgrade(&local);
    register(id, local.clone());

    // 最初の poll を予約する。UI がもう終わっていたら、future を捨てて
    // 「終わったタスク」を返す (`spawn` は fire-and-forget が主用途なので、
    // `Result` にはしない)。
    if !wake.schedule() {
        local.finish();
    }
    Task { local: handle }
}

/// The task handle returned by [`Tasks::spawn`](crate::Tasks::spawn).
///
/// **Dropping it does not stop the task.** Call [`cancel`](Task::cancel) to stop
/// it. To abort the previous work on every click, keep the handle in an
/// `Rc<RefCell<Option<Task>>>` and `cancel` it on the next click.
pub struct Task {
    local: Weak<TaskLocal>,
}

impl Task {
    /// Stops further polls and drops the future.
    ///
    /// If the future is being polled right now (called from within itself), it is
    /// dropped when that poll returns.
    pub fn cancel(self) {
        if let Some(local) = self.local.upgrade() {
            local.cancel();
        }
    }

    /// Whether it has finished (or been [`cancel`](Task::cancel)led).
    pub fn is_finished(&self) -> bool {
        match self.local.upgrade() {
            Some(local) => local.finished.get(),
            None => true,
        }
    }
}

impl std::fmt::Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Task")
            .field("finished", &self.is_finished())
            .finish()
    }
}
