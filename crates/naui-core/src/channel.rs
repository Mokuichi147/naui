//! 別スレッドから UI スレッドへ値を渡すチャネル。
//!
//! 送る側 ([`Sender`] が持つ [`ChannelShared`]) と受け取る側 ([`ChannelLocal`]) を
//! 型として分けてある。前者は `Send + Sync`、後者は受信クロージャ
//! (`FnMut(T)`、`Send` でなくてよい) を持つので UI スレッド専用。
//! スレッドをまたぐのは値と番号だけ。

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::main_thread::{
    call_guarded, deregister, next_id, pump_by_id, register, AppDispatch, LocalEntry,
};
use crate::{Result, Slot};

/// 送る側が共有する状態。**別スレッドへ渡る唯一の部分。**
pub(crate) struct ChannelShared<T> {
    id: u64,
    queue: Mutex<VecDeque<T>>,
    /// UI スレッドへの pump を予約済みかどうか。1000 回送っても投函は 1 件で済む。
    scheduled: AtomicBool,
    /// 生きている `Sender` の数。0 になったら「閉じかけ」。
    senders: AtomicUsize,
    /// 最後の `Sender` が落ちた。残った値を配り切ったら閉じる。
    closing: AtomicBool,
    dispatch: Arc<AppDispatch>,
}

impl<T: Send + 'static> ChannelShared<T> {
    /// UI スレッドでの pump を予約する。すでに予約済みなら何もしない。
    ///
    /// 積めたら `true`。UI が終わっていたら `false` (予約は取り消す)。
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

/// 中身が毒されていても値は無事なので、そのまま取り出す。
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 受け取る側。UI スレッド専用で、登録簿が保持する。
struct ChannelLocal<T: Send + 'static> {
    shared: Arc<ChannelShared<T>>,
    handler: RefCell<Slot<dyn FnMut(T)>>,
    /// pump の再入 (コールバックの中から同じチャネルへ送った) を弾く。
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

/// [`Tasks::channel`](crate::Tasks::channel) が返す送信側。
///
/// **別スレッドへ送れる** (`Send + Sync + Clone`)。値は UI スレッドで
/// 受信クロージャへ渡される。
pub struct Sender<T: Send + 'static> {
    shared: Arc<ChannelShared<T>>,
}

impl<T: Send + 'static> Sender<T> {
    /// UI スレッドへ値を送る。
    ///
    /// **この場では受信クロージャを呼ばない。** UI スレッドが次に手が空いたときに
    /// 呼ばれる。同じチャネルへ送った値は送った順に届く。
    ///
    /// `Ok` は「値をチャネルが受け取り、UI への配送が予約済みになった」の意味で、
    /// 配送されたことの保証ではない (予約の直後に画面が閉じれば呼ばれない)。
    /// naui の UI 実行環境が終わった後は `Err` になる。
    pub fn send(&self, value: T) -> Result<()> {
        if !self.shared.dispatch.is_alive() {
            return Err(AppDispatch::closed("UI スレッドへの送信"));
        }
        self.shared.push(value);
        if !self.shared.schedule() {
            return Err(AppDispatch::closed("UI スレッドへの送信"));
        }
        Ok(())
    }

    /// UI 側がまだ受け取れる状態かどうか。
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
