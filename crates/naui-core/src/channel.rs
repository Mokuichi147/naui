//! A channel that passes values from other threads to the UI thread.
//!
//! The sending side ([`ChannelShared`], held by [`Sender`]) and the receiving side
//! ([`ChannelLocal`]) are separate types. The former is `Send + Sync`; the latter
//! holds the receiving closure (`FnMut(T)`, which need not be `Send`) and so is
//! UI-thread only. Only values and numbers cross threads.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::ThreadId;

use crate::main_thread::{
    call_guarded, deregister, next_id, pump_by_id, register, AppDispatch, LocalEntry,
};
use crate::{Result, Slot};

/// What the lock of [`ChannelShared`] protects.
struct Queue<T> {
    values: VecDeque<T>,
    /// The thread inside the receiving closure, while it is. A thread closing the
    /// channel waits for it, unless it is that thread.
    delivering: Option<ThreadId>,
}

/// State shared by the sending side. **The only part that goes to other threads.**
pub(crate) struct ChannelShared<T> {
    id: u64,
    queue: Mutex<Queue<T>>,
    /// Signalled when the receiving closure has returned.
    delivered: Condvar,
    /// Whether a pump on the UI thread is already scheduled. Sending 1000 times still
    /// posts only once.
    scheduled: AtomicBool,
    /// The number of live `Sender`s. At 0 the channel is "closing".
    senders: AtomicUsize,
    /// The last `Sender` was dropped. Close once the remaining values are delivered.
    closing: AtomicBool,
    /// Closed with a [`ChannelCloser`]: nothing more is accepted or delivered.
    closed: AtomicBool,
    dispatch: Arc<AppDispatch>,
}

impl<T: Send + 'static> ChannelShared<T> {
    /// Schedules a pump on the UI thread. Does nothing if one is already scheduled.
    ///
    /// `true` if it was queued. `false` if the UI has finished (the reservation is
    /// withdrawn).
    fn schedule(&self) -> bool {
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

    /// Queues a value. `false` if the channel is closed.
    ///
    /// The flag is read under the lock [`close`](CloseChannel::close) empties the queue
    /// under, so a value is either refused here or taken out there.
    fn push(&self, value: T) -> bool {
        let mut queue = lock(&self.queue);
        if self.is_closed() {
            return false;
        }
        queue.values.push_back(value);
        true
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    fn take_all(&self) -> VecDeque<T> {
        let mut queue = lock(&self.queue);
        std::mem::take(&mut queue.values)
    }

    fn is_empty(&self) -> bool {
        lock(&self.queue).values.is_empty()
    }

    /// Enters the receiving closure, unless the channel is closed. The flag is read
    /// under the lock [`close`](CloseChannel::close) sets it under, so either this sees
    /// it, or that sees the closure entered and waits for it to return.
    fn start_delivery(&self) -> Option<Delivery<'_, T>> {
        let mut queue = lock(&self.queue);
        if self.is_closed() {
            return None;
        }
        queue.delivering = Some(std::thread::current().id());
        Some(Delivery(self))
    }
}

/// The receiving closure being called. Dropping it says that it has returned.
struct Delivery<'a, T>(&'a ChannelShared<T>);

impl<T> Drop for Delivery<'_, T> {
    fn drop(&mut self) {
        lock(&self.0.queue).delivering = None;
        self.0.delivered.notify_all();
    }
}

/// What a [`ChannelCloser`] needs, without the channel's value type.
trait CloseChannel: Send + Sync {
    fn close(&self);
}

impl<T: Send + 'static> CloseChannel for ChannelShared<T> {
    fn close(&self) {
        let waiting = {
            let mut queue = lock(&self.queue);
            self.closed.store(true, Ordering::Release);
            let waiting = std::mem::take(&mut queue.values);
            // 別のスレッドで受信処理が動いていれば、戻るまで待つ。受信処理の中から
            // 閉じたのなら、待つと自分の終わりを待つことになるので待たない。
            while queue
                .delivering
                .is_some_and(|thread| thread != std::thread::current().id())
            {
                queue = self
                    .delivered
                    .wait(queue)
                    .unwrap_or_else(|e| e.into_inner());
            }
            waiting
        };
        // ロックの外で捨てる。値の Drop がこのチャネルへ送っても固まらないように。
        drop(waiting);
        // 受信側は UI スレッドのレジストリにあるので、外すのは pump に任せる。
        self.schedule();
    }
}

/// Even if the mutex is poisoned the value is intact, so take it out anyway.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The receiving side. UI-thread only; held by the registry.
struct ChannelLocal<T: Send + 'static> {
    shared: Arc<ChannelShared<T>>,
    handler: RefCell<Slot<dyn FnMut(T)>>,
    /// Rejects re-entering the pump (sending to the same channel from inside the
    /// callback).
    busy: Cell<bool>,
}

impl<T: Send + 'static> LocalEntry for ChannelLocal<T> {
    fn pump(&self) {
        if self.busy.replace(true) {
            return;
        }
        // 入っている分だけ処理する。処理の途中で増えた分は次のティックへ回す。
        // 処理の途中で閉じられたら、残りは渡さない。
        // 残りの Drop が panic しても片付けが飛ばないよう、捨てるのはこの関数の最後。
        let mut values = self.shared.take_all().into_iter();
        while values.len() > 0 {
            let Some(_delivery) = self.shared.start_delivery() else {
                break;
            };
            if let Some(value) = values.next() {
                call_guarded(&self.handler, value);
            }
        }
        self.busy.set(false);

        // 予約を解いてから、その隙に別スレッドが積んでいないか確かめる。
        // 順序を逆にすると、この間に来た値を取りこぼす。
        self.shared.scheduled.store(false, Ordering::Release);
        if self.shared.is_closed() {
            deregister(self.shared.id);
        } else if !self.shared.is_empty() {
            self.shared.schedule();
        } else if self.shared.closing.load(Ordering::Acquire) {
            // 送り手がもういない。配り切ったのでここで閉じる。
            deregister(self.shared.id);
        }
    }
}

pub(crate) fn open<T, F>(dispatch: &Arc<AppDispatch>, on_value: F) -> Sender<T>
where
    T: Send + 'static,
    F: FnMut(T) + 'static,
{
    let id = next_id();
    let shared = Arc::new(ChannelShared {
        id,
        queue: Mutex::new(Queue {
            values: VecDeque::new(),
            delivering: None,
        }),
        delivered: Condvar::new(),
        scheduled: AtomicBool::new(false),
        senders: AtomicUsize::new(1),
        closing: AtomicBool::new(false),
        closed: AtomicBool::new(false),
        dispatch: Arc::clone(dispatch),
    });
    let local = ChannelLocal {
        shared: Arc::clone(&shared),
        handler: RefCell::new(Some(Box::new(on_value))),
        busy: Cell::new(false),
    };
    register(id, std::rc::Rc::new(local));
    Sender { shared }
}

/// The sending side returned by [`Tasks::channel`](crate::Tasks::channel).
///
/// **Can be sent to other threads** (`Send + Sync + Clone`). Values are handed to
/// the receiving closure on the UI thread.
pub struct Sender<T: Send + 'static> {
    shared: Arc<ChannelShared<T>>,
}

impl<T: Send + 'static> Sender<T> {
    /// Sends a value to the UI thread.
    ///
    /// **The receiving closure is not called on the spot.** It is called the next time
    /// the UI thread is free. Values sent to the same channel arrive in the order they
    /// were sent.
    ///
    /// `Ok` means "the channel accepted the value and delivery to the UI is
    /// scheduled", not that it was delivered (if the window closes right after
    /// scheduling, the closure is not called). Returns `Err` after naui's UI runtime
    /// has finished, or once the channel has been closed with a [`ChannelCloser`].
    pub fn send(&self, value: T) -> Result<()> {
        if !self.shared.dispatch.is_alive() {
            return Err(AppDispatch::closed("send to the UI thread"));
        }
        if !self.shared.push(value) {
            return Err(crate::Error::new(
                "send to the UI thread",
                "the channel has been closed",
            ));
        }
        if !self.shared.schedule() {
            return Err(AppDispatch::closed("send to the UI thread"));
        }
        Ok(())
    }

    /// Whether the UI side can still receive. Work that takes a while can check it and
    /// stop once nobody is waiting for its result.
    pub fn is_open(&self) -> bool {
        self.shared.dispatch.is_alive() && !self.shared.is_closed()
    }

    /// A handle that closes this channel, for whoever started the work and may stop
    /// wanting its results. It does not count as a sender, so it does not keep the
    /// channel open.
    pub fn closer(&self) -> ChannelCloser {
        let shared: Arc<dyn CloseChannel> = self.shared.clone();
        ChannelCloser { shared }
    }
}

impl<T: Send + 'static> Clone for Sender<T> {
    fn clone(&self) -> Self {
        self.shared.senders.fetch_add(1, Ordering::AcqRel);
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<T: Send + 'static> Drop for Sender<T> {
    fn drop(&mut self) {
        if self.shared.senders.fetch_sub(1, Ordering::AcqRel) != 1 {
            return;
        }
        // 送り手がいなくなった。ただしキューに残った値はまだ配る
        // (`send` してすぐ `drop` したワーカーの最後の通知を落とさないため)。
        self.shared.closing.store(true, Ordering::Release);
        self.shared.schedule();
    }
}

/// Closes a channel, from [`Sender::closer`].
///
/// [`close`](Self::close) cuts the channel for every [`Sender`] at once: values not
/// yet delivered are dropped and the receiving closure is not called again, even for
/// a value it is in the middle of a batch of. From then on [`Sender::send`] returns
/// `Err` and [`Sender::is_open`] returns `false`, so the sending side can tell. The
/// receiving closure itself is released the next time the UI thread is free.
///
/// It can be sent to other threads. Closing twice does nothing more.
///
/// When `close` has returned, the receiving closure is not running and will not start.
/// So on another thread it waits for a call in progress to return: do not close while
/// holding what the closure needs, or from a thread the closure is waiting for. Called
/// from inside the closure it does not wait, and that call runs to its end.
#[derive(Clone)]
pub struct ChannelCloser {
    shared: Arc<dyn CloseChannel>,
}

impl ChannelCloser {
    /// Closes the channel.
    pub fn close(&self) {
        self.shared.close();
    }
}

impl std::fmt::Debug for ChannelCloser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelCloser").finish_non_exhaustive()
    }
}

impl<T: Send + 'static> std::fmt::Debug for Sender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sender")
            .field("open", &self.is_open())
            .finish()
    }
}
