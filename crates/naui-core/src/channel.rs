//! A channel that passes values from other threads to the UI thread.
//!
//! The sending side ([`ChannelShared`], held by [`Sender`]) and the receiving side
//! ([`ChannelLocal`]) are separate types. The former is `Send + Sync`; the latter
//! holds the receiving closure (`FnMut(T)`, which need not be `Send`) and so is
//! UI-thread only. Only values and numbers cross threads.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::main_thread::{
    call_guarded, deregister, next_id, pump_by_id, register, AppDispatch, LocalEntry,
};
use crate::{Result, Slot};

/// State shared by the sending side. **The only part that goes to other threads.**
pub(crate) struct ChannelShared<T> {
    id: u64,
    queue: Mutex<VecDeque<T>>,
    /// Whether a pump on the UI thread is already scheduled. Sending 1000 times still
    /// posts only once.
    scheduled: AtomicBool,
    /// The number of live `Sender`s. At 0 the channel is "closing".
    senders: AtomicUsize,
    /// The last `Sender` was dropped. Close once the remaining values are delivered.
    closing: AtomicBool,
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

    fn push(&self, value: T) {
        let mut queue = lock(&self.queue);
        queue.push_back(value);
    }

    fn take_all(&self) -> VecDeque<T> {
        let mut queue = lock(&self.queue);
        std::mem::take(&mut *queue)
    }

    fn is_empty(&self) -> bool {
        lock(&self.queue).is_empty()
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
        for value in self.shared.take_all() {
            call_guarded(&self.handler, value);
        }
        self.busy.set(false);

        // 予約を解いてから、その隙に別スレッドが積んでいないか確かめる。
        // 順序を逆にすると、この間に来た値を取りこぼす。
        self.shared.scheduled.store(false, Ordering::Release);
        if !self.shared.is_empty() {
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
        queue: Mutex::new(VecDeque::new()),
        scheduled: AtomicBool::new(false),
        senders: AtomicUsize::new(1),
        closing: AtomicBool::new(false),
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
    /// has finished.
    pub fn send(&self, value: T) -> Result<()> {
        if !self.shared.dispatch.is_alive() {
            return Err(AppDispatch::closed("send to the UI thread"));
        }
        self.shared.push(value);
        if !self.shared.schedule() {
            return Err(AppDispatch::closed("send to the UI thread"));
        }
        Ok(())
    }

    /// Whether the UI side can still receive.
    pub fn is_open(&self) -> bool {
        self.shared.dispatch.is_alive()
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

impl<T: Send + 'static> std::fmt::Debug for Sender<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sender")
            .field("open", &self.is_open())
            .finish()
    }
}
