//! UI スレッドの上で future を回す小さな実行器。
//!
//! `std::task::Wake` を使うので、`RawWakerVTable` を手書きせずに済み、
//! このクレートの `forbid(unsafe_code)` を保ったまま書ける。
//!
//! `Waker` は `Send + Sync` でなければならない一方、future 自体は `!Send` の
//! ままにしたい (ウィジェットを持ち込めるように)。そこで
//! 「起こす側」([`TaskWake`]) と「実体」([`TaskLocal`]) を型として分け、
//! スレッドをまたぐのは番号だけにしてある。

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use crate::main_thread::{deregister, next_id, pump_by_id, register, AppDispatch, LocalEntry};

/// 起こす側。`Waker` の中身なので `Send + Sync` でなければならない。
struct TaskWake {
    id: u64,
    /// 次の poll を予約済みかどうか。多重 wake を 1 回にまとめる。
    scheduled: AtomicBool,
    /// まだ回っているか。完了・cancel の後に古い `Waker` から起こされても、
    /// ここが `false` なら投函しない。
    active: AtomicBool,
    dispatch: Arc<AppDispatch>,
}

impl TaskWake {
    /// 次の poll を予約する。積めたら `true`。
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

/// 実体。UI スレッド専用で、登録簿が保持する。
struct TaskLocal {
    id: u64,
    future: RefCell<Option<Pin<Box<dyn Future<Output = ()>>>>>,
    cancelled: Cell<bool>,
    finished: Cell<bool>,
    /// poll している最中かどうか (poll の中から cancel されたときの判別用)。
    polling: Cell<bool>,
    wake: Arc<TaskWake>,
    waker: RefCell<Option<Waker>>,
}

impl TaskLocal {
    /// 終わりの後始末。future を捨て、古い `Waker` からの投函を止め、登録簿から外す。
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

/// [`Tasks::spawn`](crate::Tasks::spawn) が返すタスクの取っ手。
///
/// **落としてもタスクは止まらない。** 止めたいときは [`cancel`](Task::cancel) を呼ぶ。
/// 押すたびに前の処理を打ち切りたいときは、`Rc<RefCell<Option<Task>>>` に
/// 取っ手を持っておいて、次に押されたときに `cancel` する。
pub struct Task {
    local: Weak<TaskLocal>,
}

impl Task {
    /// 次の poll をやめ、future を捨てる。
    ///
    /// future が今まさに poll されている最中 (自分自身から呼んだ場合) は、
    /// その poll から戻った時点で捨てられる。
    pub fn cancel(self) {
        if let Some(local) = self.local.upgrade() {
            local.cancel();
        }
    }

    /// 終わった (または [`cancel`](Task::cancel) された) かどうか。
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
