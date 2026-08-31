//! UI スレッドへ仕事を積むための土台。
//!
//! バックエンドが供給するのは [`MainThread`] 1 つだけで、
//! チャネル ([`crate::channel`]) とタスク ([`crate::task`]) はどちらもこの上に載る。
//!
//! 実体 (受信クロージャや future) は `Rc` を持つので UI スレッドの
//! `thread_local!` 登録簿に置き、スレッドをまたぐのは `u64` の番号だけにする。
//! ネイティブのウィジェットも `!Send` な future も、一度も UI スレッドの外へ出ない。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::channel::Sender;
use crate::task::Task;
use crate::{Error, Slot};

/// UI スレッドで 1 回だけ実行される仕事。
pub type Work = Box<dyn FnOnce() + Send + 'static>;

/// バックエンドが供給する「UI スレッドで 1 回だけ実行する」仕組み。
///
/// **同期実行してはならない。** どの環境でも必ず後回しにすること。
/// `send` や `spawn` がその場でコールバックや poll を起こさないという
/// naui の約束は、この一点に依っている。
///
/// アプリが実装するものではない。
pub trait MainThread: Send + Sync + 'static {
    /// 積めたら `true`。イベントループが終わっている等で届かないなら `false`。
    fn post(&self, work: Work) -> bool;
}

/// 投函先と「UI がまだ生きているか」をまとめたもの。
///
/// `TryEnqueue` は終了後に `Ok(false)` を返すが、macOS の main queue と
/// GTK の idle source は同じようには失敗を返さない。4 環境で意味をそろえるため、
/// core 側で `alive` を先に見る。
pub(crate) struct AppDispatch {
    alive: AtomicBool,
    main_thread: Arc<dyn MainThread>,
}

impl AppDispatch {
    fn new(main_thread: Arc<dyn MainThread>) -> Self {
        Self {
            alive: AtomicBool::new(true),
            main_thread,
        }
    }

    pub(crate) fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    fn shutdown(&self) {
        self.alive.store(false, Ordering::Release);
    }

    /// UI スレッドへ仕事を積む。終了済みなら積まずに `false`。
    pub(crate) fn post(&self, work: Work) -> bool {
        if !self.is_alive() {
            return false;
        }
        self.main_thread.post(work)
    }

    /// 終了したことを伝えるエラー。
    pub(crate) fn closed(context: &'static str) -> Error {
        Error::new(context, "UI はもう受け取れません")
    }
}

/// UI スレッドで番号から引き直して起こせるもの。
///
/// `ChannelLocal<T>` は `T` がチャネルごとに違うので、単一の登録簿へ入れるには
/// この形で型を消す必要がある。
pub(crate) trait LocalEntry {
    fn pump(&self);
}

thread_local! {
    /// UI スレッドで生きているチャネル / タスクの登録簿。
    static REGISTRY: RefCell<HashMap<u64, Rc<dyn LocalEntry>>> =
        RefCell::new(HashMap::new());
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
}

pub(crate) fn next_id() -> u64 {
    NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id.wrapping_add(1));
        id
    })
}

pub(crate) fn register(id: u64, entry: Rc<dyn LocalEntry>) {
    let _ = REGISTRY.try_with(|r| r.borrow_mut().insert(id, entry));
}

/// 登録簿から外す。取り出した `Rc` は借用を解いてから落とす
/// (drop の中で登録簿を触られても壊れないように)。
pub(crate) fn deregister(id: u64) {
    let removed = REGISTRY.try_with(|r| r.borrow_mut().remove(&id));
    drop(removed);
}

/// 番号から実体を引いて起こす。
///
/// **登録簿を借りたままコールバックを呼んではいけない。** コールバックの中から
/// 別のチャネルを開く / 閉じるといった再入で `RefCell` が panic するため、
/// `Rc` を clone して借用を解いてから呼ぶ。
pub(crate) fn pump_by_id(id: u64) {
    let entry = REGISTRY
        .try_with(|r| r.borrow().get(&id).cloned())
        .ok()
        .flatten();
    if let Some(entry) = entry {
        entry.pump();
    }
}

fn clear_registry() {
    let entries = REGISTRY.try_with(|r| std::mem::take(&mut *r.borrow_mut()));
    drop(entries);
}

/// クロージャを取り出して呼び、差し替えられていなければ戻す。
///
/// 呼び出し中に同じウィジェットを触られても二重借用にならないための、
/// naui で共通のイディオム。panic はここで止める (どのバックエンドでも
/// C / WinRT の境界を越えて巻き戻せないため)。panic の内容そのものは
/// 既定の panic hook が stderr へ出すので、ここでは何も出力しない。
pub(crate) fn call_guarded<T>(slot: &RefCell<Slot<dyn FnMut(T)>>, value: T) {
    let taken = slot.borrow_mut().take();
    let Some(mut f) = taken else {
        return;
    };
    let _ = catch_unwind(AssertUnwindSafe(|| f(value)));
    let mut slot = slot.borrow_mut();
    if slot.is_none() {
        *slot = Some(f);
    }
}

/// UI スレッドで動く仕事を積むためのハンドル。
///
/// [`Ui::tasks`](../../naui/struct.Ui.html#method.tasks) で作る。
/// clone してコールバックへ持ち込めるが、**別スレッドへは送れない** (`!Send`)。
///
/// 別スレッドの結果を画面へ返すには [`channel`](Tasks::channel)、
/// UI スレッドで future を回すには [`spawn`](Tasks::spawn) を使う。
#[derive(Clone)]
pub struct Tasks {
    dispatch: Arc<AppDispatch>,
    /// `Rc` を持たせて `!Send` にする (`Arc` だけだと `Send` が付いてしまう)。
    _local: Rc<()>,
}

impl Tasks {
    /// バックエンドが `Ui::new` から呼ぶ。アプリが使うものではない。
    #[doc(hidden)]
    pub fn from_main_thread(main_thread: Arc<dyn MainThread>) -> Self {
        Self {
            dispatch: Arc::new(AppDispatch::new(main_thread)),
            _local: Rc::new(()),
        }
    }

    /// UI の実行環境が終わったことを伝える。バックエンドの終了処理から呼ぶ。
    ///
    /// 以降の [`Sender::send`] は失敗し、登録簿に残っている受信クロージャと
    /// future は解放される。アプリが使うものではない。
    #[doc(hidden)]
    pub fn shutdown(&self) {
        self.dispatch.shutdown();
        clear_registry();
    }

    /// 別スレッドから値を受け取るチャネルを開く。
    ///
    /// `on_value` は **UI スレッドで**呼ばれるので、中でウィジェットを触ってよい。
    /// 返る [`Sender`] は `Send + Sync + Clone` なので、好きなだけスレッドへ渡せる。
    ///
    /// 値は**送った順に**届く (チャネルが複数あるとき、チャネルをまたいだ順序は
    /// 決まっていない)。[`send`](Sender::send) はその場でクロージャを呼ばず、
    /// UI スレッドの手が空いてから届く。
    pub fn channel<T, F>(&self, on_value: F) -> Sender<T>
    where
        T: Send + 'static,
        F: FnMut(T) + 'static,
    {
        crate::channel::open(&self.dispatch, on_value)
    }

    /// UI スレッドで future を回す。
    ///
    /// future は `Send` でなくてよいので、ウィジェットのハンドルを持ち込める。
    /// 進めるのは UI スレッドのイベントループなので、**中でブロッキング処理を
    /// すると画面が止まる**。重い処理は [`channel`](Tasks::channel) へ出す。
    ///
    /// 戻り値を捨ててもタスクは走り続ける。止めたいときは [`Task::cancel`]。
    pub fn spawn<F>(&self, future: F) -> Task
    where
        F: std::future::Future<Output = ()> + 'static,
    {
        crate::task::spawn(&self.dispatch, future)
    }
}

impl std::fmt::Debug for Tasks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tasks")
            .field("alive", &self.dispatch.is_alive())
            .finish()
    }
}
