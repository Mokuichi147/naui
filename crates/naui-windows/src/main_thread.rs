//! 別スレッドから UI スレッドへ仕事を投げる口。
//!
//! `DispatcherQueue` は agile (`unsafe impl Send/Sync`) なので、UI スレッドで
//! 取ったものを別スレッドから `TryEnqueue` してよい。メディアの再生通知
//! (`crate::media`) が既に同じ経路を使っている。

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use naui_core::{MainThread, Work};
use naui_winui3::Microsoft::UI::Dispatching::{
    DispatcherQueue, DispatcherQueueHandler, DispatcherQueueTimer,
};
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows_core::IInspectable;

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

    /// UI スレッドへ移ってから、1 回きりの `DispatcherQueueTimer` を張る。
    fn post_after(&self, delay: Duration, work: Work) -> bool {
        self.post(Box::new(move || start_timer(delay, work)))
    }
}

/// `TimeSpan` の 1 ミリ秒 (100 ナノ秒きざみ)。
const TICKS_PER_MILLI: i64 = 10_000;

/// UI スレッドで呼ぶ。張れなかったときはその場で実行する (遅れるより
/// 呼ばれないほうが困るため)。
fn start_timer(delay: Duration, work: Work) {
    let timer = DispatcherQueue::GetForCurrentThread().and_then(|queue| queue.CreateTimer());
    let Ok(timer) = timer else {
        let _ = catch_unwind(AssertUnwindSafe(work));
        return;
    };
    let millis = i64::try_from(delay.as_millis()).unwrap_or(i64::MAX / TICKS_PER_MILLI);
    let interval = TimeSpan {
        Duration: millis.saturating_mul(TICKS_PER_MILLI),
    };
    // タイマーは鳴るまで手元で持つ必要があるので、デリゲートの中へ預け、
    // 鳴ったら取り出して止める (循環もそこで切れる)。
    let slot = Mutex::new(Some(work));
    let keep: Arc<Mutex<Option<DispatcherQueueTimer>>> = Arc::new(Mutex::new(None));
    let handler = TypedEventHandler::<DispatcherQueueTimer, IInspectable>::new({
        let keep = keep.clone();
        move |_, _| {
            if let Some(timer) = keep.lock().ok().and_then(|mut keep| keep.take()) {
                let _ = timer.Stop();
            }
            if let Some(work) = slot.lock().ok().and_then(|mut slot| slot.take()) {
                let _ = catch_unwind(AssertUnwindSafe(work));
            }
            Ok(())
        }
    });
    let started = timer.SetInterval(interval).is_ok()
        && timer.SetIsRepeating(false).is_ok()
        && timer.Tick(&handler).is_ok()
        && timer.Start().is_ok();
    if started {
        if let Ok(mut keep) = keep.lock() {
            *keep = Some(timer);
        }
    }
}
