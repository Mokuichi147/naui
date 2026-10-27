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

use naui::{Result, Task, Ui};

use crate::parts::{self, Notice};

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
            let mut state = self.0.lock().expect("oneshot lock");
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
        let mut state = self.0.lock().expect("oneshot lock");
        if state.finished {
            return Poll::Ready(());
        }
        state.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

/// 別スレッドからの受け渡しと、UI スレッドで回す非同期処理。
pub(crate) fn build(ui: &Ui, notice: &Notice) -> Result<naui::Stack> {
    let pane = parts::pane(ui)?;

    // 重い処理を別のスレッドへ出し、途中経過だけを画面へ返す。3 段階に分けて
    // 進むので、進捗バーが 1 段ずつ動く。
    parts::section(ui, &pane, "Worker thread")?;
    // ブラウザにはスレッドが無い。待ち時間を作れず、動かない画面に見えて
    // しまうので、そのことだけは画面にも書く。
    #[cfg(target_arch = "wasm32")]
    {
        let note = ui.label("Browsers have no threads, so every task here finishes at once.")?;
        note.set_color(naui::TextColor::Secondary);
        pane.append(&note);
    }

    let worker_status = parts::readout(ui, "Idle")?;
    let progress = ui.progress_bar()?;
    progress.set_sizing(naui::Sizing::fill_width());
    // 走っている間だけ回す。止めている間も場所は残るので、行の並びは動かない。
    let busy = ui.spinner()?;
    busy.set_spinning(false);
    let start = ui.button("Start heavy work")?;
    start.on_click({
        let tasks = ui.tasks();
        let worker_status = worker_status.clone();
        let progress = progress.clone();
        let busy = busy.clone();
        let start = start.downgrade();
        move || {
            // 走っている間は押せないようにする (前の処理と混ざらないように)。
            if let Some(start) = start.upgrade() {
                start.set_enabled(false);
            }
            worker_status.set_text("Running… 0/3");
            progress.set_value(0.0);
            busy.set_spinning(true);

            // 受け取る側は UI スレッドに置く。ここでウィジェットを触ってよい。
            let sender = tasks.channel({
                let worker_status = worker_status.clone();
                let progress = progress.clone();
                let busy = busy.clone();
                let start = start.clone();
                move |p: Progress| {
                    progress.set_value(p.done as f64 / p.total as f64);
                    if p.done == p.total {
                        worker_status.set_text("Done");
                        busy.set_spinning(false);
                        if let Some(start) = start.upgrade() {
                            start.set_enabled(true);
                        }
                    } else {
                        worker_status.set_text(&format!("Running… {}/{}", p.done, p.total));
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

    // 「待っている間も画面は止まっていない」ことを、その場で確かめられるよう、
    // 始めるボタンの隣に押せるボタンを置く。
    let taps = Rc::new(Cell::new(0usize));
    let tap = ui.button("Still responsive?")?;
    tap.on_click({
        let taps = taps.clone();
        let notice = notice.clone();
        move || {
            let next = taps.get() + 1;
            taps.set(next);
            notice.show(&format!("Tapped {next} times"));
        }
    });
    let worker_row = parts::row(ui)?;
    worker_row.append(&start);
    worker_row.append(&tap);
    worker_row.append(&busy);
    worker_row.append(&worker_status);
    pane.append(&worker_row);
    pane.append(&progress);

    // ボタンのクロージャの中から async を始め、別スレッドの結果を待つ。
    // 待っている間に「Stop」を押すと、続きは実行されない。
    parts::section(ui, &pane, "Async task")?;

    let async_status = parts::readout(ui, "Idle")?;
    // 走っているものを覚えておき、次に押されたら止める。
    let running: Rc<RefCell<Option<Task>>> = Rc::new(RefCell::new(None));

    let load = ui.button("Start loading")?;
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
                    async_status.set_text("Loading… 1/2");
                    first.await;
                    async_status.set_text("Loading… 2/2");
                    second.await;
                    async_status.set_text("Done");
                }
            });
            *running.borrow_mut() = Some(task);
        }
    });

    let stop = ui.button("Stop")?;
    stop.on_click({
        let async_status = async_status.clone();
        let running = running.clone();
        move || {
            if let Some(task) = running.borrow_mut().take() {
                task.cancel();
                async_status.set_text("Stopped");
            }
        }
    });

    let async_row = parts::row(ui)?;
    async_row.append(&load);
    async_row.append(&stop);
    async_row.append(&async_status);
    pane.append(&async_row);

    Ok(pane)
}
