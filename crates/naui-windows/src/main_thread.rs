//! 別スレッドから UI スレッドへ仕事を投げる口。
//!
//! `DispatcherQueue` は agile (`unsafe impl Send/Sync`) なので、UI スレッドで
//! 取ったものを別スレッドから `TryEnqueue` してよい。メディアの再生通知
//! (`crate::media`) が既に同じ経路を使っている。

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

use naui_core::{MainThread, Work};
use winui3::Microsoft::UI::Dispatching::{DispatcherQueue, DispatcherQueueHandler};

/// UI スレッドの `DispatcherQueue`。
///
/// 取得に失敗したときは `None` にしておき、`post` が常に失敗を返すようにする
/// (`Ui::new` を `Result` にしないため)。
pub(crate) struct Dispatcher(Option<DispatcherQueue>);

impl Dispatcher {
    /// **UI スレッドから呼ぶこと。**
    pub(crate) fn for_current_thread() -> Self {
        Self(DispatcherQueue::GetForCurrentThread().ok())
    }
}

impl MainThread for Dispatcher {
    fn post(&self, work: Work) -> bool {
        let Some(queue) = self.0.as_ref() else {
            return false;
        };
        // WinRT のデリゲートは `Fn` なので、一度きりの仕事はセルに預けて取り出す。
        // `UiThreadCell` はスレッドが違うと取り出せないため、ここでは使えない。
        let slot = Mutex::new(Some(work));
        let handler = DispatcherQueueHandler::new(move || {
            let taken = slot.lock().ok().and_then(|mut slot| slot.take());
            let Some(work) = taken else {
                return Ok(());
            };
            // WinRT のデリゲートから panic を巻き戻すと、ABI の境界を越えて
            // アクセス違反になる。内容は既定の panic hook が stderr へ出す。
            let _ = catch_unwind(AssertUnwindSafe(work));
            Ok(())
        });
        // 終了後は `Ok(false)` が返る。`Err` と合わせて「積めなかった」とみなす。
        matches!(queue.TryEnqueue(&handler), Ok(true))
    }
}
