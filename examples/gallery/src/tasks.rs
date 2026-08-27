//! 別スレッドとの受け渡し (`Tasks::channel`) と、UI スレッドで回す非同期処理
//! (`Tasks::spawn`)。
//!
//! どちらも「押した瞬間には何も起きず、あとから画面が変わる」ことと、
//! 「待っている間も画面が止まらない」ことを確かめられるようにしてある。

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use naui::{Orientation, Padding, Result, Task, Ui};

/// ワーカーが送ってくる進捗。
///
/// スレッドをまたぐので、運ぶのは**値だけ**。ウィジェットは渡さない
/// (渡す必要もない。画面の書き換えは UI スレッド側のクロージャがやる)。
struct Progress {
    done: usize,
    total: usize,
}

const STEPS: usize = 3;

/// 重い処理のつもりで少し待つ。
///
/// **Web (wasm) にはスレッドが無いので待たない。** そちらではどのボタンも
/// 待ち時間なしで終わる。
fn pretend_to_work() {
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::sleep(std::time::Duration::from_millis(600));
}

/// 画面と別のところで処理を走らせる。
///
/// ネイティブは本物のスレッド。Web にはスレッドが無いので、その場で走らせる
/// (`pretend_to_work` が待たないので画面は止まらない)。
fn run_off_thread(work: impl FnOnce() + Send + 'static) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::spawn(work);
    }
    #[cfg(target_arch = "wasm32")]
    {
        work();
    }
}

/// 別スレッドの完了を `await` で待つための、一度きりの待ち合わせ。
///
/// naui が用意しているのは「コールバックで受け取る」`Tasks::channel` だけだが、
/// `Waker` は `Send` なので、**待ち合わせはこのようにアプリ側で書ける**
/// (起こされた `Waker` が UI スレッドへ poll を積み直してくれる)。
#[derive(Clone)]
struct Oneshot(Arc<Mutex<OneshotState>>);

#[derive(Default)]
struct OneshotState {
    finished: bool,
    waker: Option<Waker>,
}

impl Oneshot {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(OneshotState::default())))
    }

    /// 別スレッドから呼ぶ。待っている処理を起こす。
    fn finish(&self) {
        let waker = {
            let mut state = self.0.lock().expect("待ち合わせ");
            state.finished = true;
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl Future for Oneshot {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut state = self.0.lock().expect("待ち合わせ");
        if state.finished {
            return Poll::Ready(());
        }
        state.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

/// 別スレッドからの受け渡しと、UI スレッドで回す非同期処理。
pub(crate) fn build(ui: &Ui) -> Result<naui::Stack> {
    let pane = ui.stack(Orientation::Vertical)?;
    pane.set_spacing(12.0);
    pane.set_padding(Padding::all(12.0));

    pane.append(&ui.label("別スレッドからの受け渡し")?);
    pane.append(&ui.label("重い処理を別のスレッドへ出し、途中経過だけを画面へ返します。")?);
    pane.append(&ui.label("3 段階に分けて進むので、進捗バーが 1 段ずつ動きます。")?);
    // ブラウザにはスレッドが無い。待ち時間を作れないので、そのことを画面にも書く。
    #[cfg(target_arch = "wasm32")]
    pane.append(
        &ui.label(
            "※ ブラウザにはスレッドが無いため、この画面ではどれも待ち時間なしで終わります。",
        )?,
    );

    let worker_status = ui.label("待機中")?;
    let progress = ui.progress_bar()?;
    let start = ui.button("重い処理を始める")?;
    start.on_click({
        let tasks = ui.tasks();
        let worker_status = worker_status.clone();
        let progress = progress.clone();
        let start = start.clone();
        move || {
            // 走っている間は押せないようにする (前の処理と混ざらないように)。
            start.set_enabled(false);
            worker_status.set_text("実行中… 0/3");
            progress.set_value(0.0);

            // 受け取る側は UI スレッドに置く。ここでウィジェットを触ってよい。
            let sender = tasks.channel({
                let worker_status = worker_status.clone();
                let progress = progress.clone();
                let start = start.clone();
                move |p: Progress| {
                    progress.set_value(p.done as f64 / p.total as f64);
                    if p.done == p.total {
                        worker_status.set_text("完了");
                        start.set_enabled(true);
                    } else {
                        worker_status.set_text(&format!("実行中… {}/{}", p.done, p.total));
                    }
                }
            });

            // 送る側だけをスレッドへ渡す。
            run_off_thread(move || {
                for done in 1..=STEPS {
                    pretend_to_work();
                    let _ = sender.send(Progress { done, total: STEPS });
                }
            });
        }
    });

    let worker_row = ui.stack(Orientation::Horizontal)?;
    worker_row.set_spacing(8.0);
    worker_row.append(&start);
    worker_row.append(&worker_status);
    pane.append(&worker_row);
    pane.append(&progress);

    // 「待っている間も画面は止まっていない」ことを、その場で確かめられるようにする。
    pane.append(&ui.label("処理の間も画面は止まりません。下のボタンで確かめてください。")?);
    let taps = Rc::new(Cell::new(0usize));
    let tap_status = ui.label("押した回数: 0")?;
    let tap = ui.button("反応を確かめる")?;
    tap.on_click({
        let taps = taps.clone();
        let tap_status = tap_status.clone();
        move || {
            let next = taps.get() + 1;
            taps.set(next);
            tap_status.set_text(&format!("押した回数: {next}"));
        }
    });
    let tap_row = ui.stack(Orientation::Horizontal)?;
    tap_row.set_spacing(8.0);
    tap_row.append(&tap);
    tap_row.append(&tap_status);
    pane.append(&tap_row);

    pane.append(&ui.label("非同期処理")?);
    pane.append(
        &ui.label("ボタンのクロージャの中から async を始め、別スレッドの結果を待ちます。")?,
    );
    pane.append(&ui.label("待っている間に「止める」を押すと、続きは実行されません。")?);

    let async_status = ui.label("待機中")?;
    // 走っているものを覚えておき、次に押されたら止める。
    let running: Rc<RefCell<Option<Task>>> = Rc::new(RefCell::new(None));

    let load = ui.button("読み込みを始める")?;
    load.on_click({
        let tasks = ui.tasks();
        let async_status = async_status.clone();
        let running = running.clone();
        move || {
            // 前のものが残っていたら止める。取っ手を捨てるだけでは止まらない。
            if let Some(previous) = running.borrow_mut().take() {
                previous.cancel();
            }

            let first = Oneshot::new();
            let second = Oneshot::new();
            run_off_thread({
                let first = first.clone();
                let second = second.clone();
                move || {
                    pretend_to_work();
                    first.finish();
                    pretend_to_work();
                    second.finish();
                }
            });

            let task = tasks.spawn({
                let async_status = async_status.clone();
                async move {
                    async_status.set_text("読み込み中… 1/2");
                    first.await;
                    async_status.set_text("読み込み中… 2/2");
                    second.await;
                    async_status.set_text("完了");
                }
            });
            *running.borrow_mut() = Some(task);
        }
    });

    let stop = ui.button("止める")?;
    stop.on_click({
        let async_status = async_status.clone();
        let running = running.clone();
        move || {
            if let Some(task) = running.borrow_mut().take() {
                task.cancel();
                async_status.set_text("止めました");
            }
        }
    });

    let async_row = ui.stack(Orientation::Horizontal)?;
    async_row.set_spacing(8.0);
    async_row.append(&load);
    async_row.append(&stop);
    async_row.append(&async_status);
    pane.append(&async_row);

    Ok(pane)
}
