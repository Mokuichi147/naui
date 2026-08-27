//! チャネルとタスクの意味論を、イベントループ抜きで確かめる。
//!
//! `MainThread` はトレイトなので、投函をため込むだけのダブルを差し込めば
//! 「いつ配送されるか」まで含めて決定論的に検証できる。

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use naui_core::{MainThread, Tasks, Work};

/// 投函された仕事をため込むだけの `MainThread`。`drain` で好きなときに走らせる。
struct Fake {
    queued: Mutex<VecDeque<Work>>,
    posts: AtomicUsize,
    accept: AtomicBool,
}

impl Fake {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            queued: Mutex::new(VecDeque::new()),
            posts: AtomicUsize::new(0),
            accept: AtomicBool::new(true),
        })
    }

    /// 入っている分だけ走らせる。走らせる途中で増えた分は次回へ。
    fn drain(&self) -> usize {
        let works: Vec<Work> = {
            let mut queued = self.queued.lock().unwrap();
            std::mem::take(&mut *queued).into()
        };
        let count = works.len();
        for work in works {
            work();
        }
        count
    }

    /// 空になるまで繰り返す。
    fn drain_all(&self) {
        for _ in 0..1000 {
            if self.drain() == 0 {
                return;
            }
        }
        panic!("投函が止まりません");
    }

    fn posts(&self) -> usize {
        self.posts.load(Ordering::Acquire)
    }
}

impl MainThread for Fake {
    fn post(&self, work: Work) -> bool {
        if !self.accept.load(Ordering::Acquire) {
            return false;
        }
        self.posts.fetch_add(1, Ordering::AcqRel);
        self.queued.lock().unwrap().push_back(work);
        true
    }
}

fn setup() -> (Arc<Fake>, Tasks) {
    let fake = Fake::new();
    let tasks = Tasks::from_main_thread(fake.clone());
    (fake, tasks)
}

/// 落ちたことを外から見られる目印。
struct Tracer(Rc<Cell<bool>>);

impl Tracer {
    fn new() -> (Self, Rc<Cell<bool>>) {
        let flag = Rc::new(Cell::new(false));
        (Self(flag.clone()), flag)
    }
}

impl Drop for Tracer {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

// --- 型の性質 -------------------------------------------------------------

#[test]
fn 送信側はスレッドをまたげる() {
    fn assert_send_sync<T: Send + Sync>() {}
    // ここが通らなくなったら、別スレッドへ渡せなくなっている。
    assert_send_sync::<naui_core::Sender<String>>();
}

// --- チャネル -------------------------------------------------------------

#[test]
fn 送った値は配送するまで届かない() {
    let (fake, tasks) = setup();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sender = tasks.channel({
        let seen = seen.clone();
        move |v: i32| seen.borrow_mut().push(v)
    });

    sender.send(1).expect("送信");
    assert!(seen.borrow().is_empty(), "その場では呼ばれない");

    fake.drain();
    assert_eq!(*seen.borrow(), vec![1]);
}

#[test]
fn 同じチャネルでは送った順に届く() {
    let (fake, tasks) = setup();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sender = tasks.channel({
        let seen = seen.clone();
        move |v: i32| seen.borrow_mut().push(v)
    });

    for v in 1..=5 {
        sender.send(v).expect("送信");
    }
    fake.drain_all();
    assert_eq!(*seen.borrow(), vec![1, 2, 3, 4, 5]);
}

#[test]
fn 別スレッドから送った値が届く() {
    let (fake, tasks) = setup();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sender = tasks.channel({
        let seen = seen.clone();
        move |v: String| seen.borrow_mut().push(v)
    });

    let worker = std::thread::spawn(move || sender.send("完了".to_string()));
    worker.join().expect("ワーカー").expect("送信");

    assert!(seen.borrow().is_empty(), "配送するまでは届かない");
    fake.drain_all();
    assert_eq!(*seen.borrow(), vec!["完了".to_string()]);
}

#[test]
fn ハンドラの中から同じチャネルへ送れる() {
    let (fake, tasks) = setup();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let again: Rc<RefCell<Option<naui_core::Sender<i32>>>> = Rc::new(RefCell::new(None));
    let sender = tasks.channel({
        let seen = seen.clone();
        let again = again.clone();
        move |v: i32| {
            seen.borrow_mut().push(v);
            if v < 3 {
                let next = again.borrow().as_ref().cloned();
                if let Some(next) = next {
                    next.send(v + 1).expect("送信");
                }
            }
        }
    });
    *again.borrow_mut() = Some(sender.clone());

    sender.send(1).expect("送信");
    fake.drain_all();
    assert_eq!(*seen.borrow(), vec![1, 2, 3]);
}

#[test]
fn ハンドラの中から別のチャネルを開いても壊れない() {
    let (fake, tasks) = setup();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let opened = Rc::new(RefCell::new(Vec::new()));
    let sender = tasks.channel({
        let seen = seen.clone();
        let opened = opened.clone();
        let tasks = tasks.clone();
        move |v: i32| {
            seen.borrow_mut().push(v);
            // 登録簿を借りたまま呼ばれていたら、ここで RefCell が panic する。
            let inner = tasks.channel(|_: u8| {});
            opened.borrow_mut().push(inner);
        }
    });

    sender.send(7).expect("送信");
    fake.drain_all();
    assert_eq!(*seen.borrow(), vec![7]);
    assert_eq!(opened.borrow().len(), 1);

    // 開いたものを閉じる側も同じ経路を通る。
    opened.borrow_mut().clear();
    fake.drain_all();
}

#[test]
fn ハンドラが落ちても配送は続く() {
    let (fake, tasks) = setup();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sender = tasks.channel({
        let seen = seen.clone();
        move |v: i32| {
            if v == 2 {
                panic!("わざと落とす");
            }
            seen.borrow_mut().push(v);
        }
    });

    for v in 1..=3 {
        sender.send(v).expect("送信");
    }
    fake.drain_all();
    assert_eq!(*seen.borrow(), vec![1, 3], "落ちた値だけ飛ばして続く");
}

#[test]
fn 連続して送っても投函はまとまる() {
    let (fake, tasks) = setup();
    let seen = Rc::new(Cell::new(0usize));
    let sender = tasks.channel({
        let seen = seen.clone();
        move |_: u32| seen.set(seen.get() + 1)
    });

    for v in 0..1000 {
        sender.send(v).expect("送信");
    }
    assert_eq!(fake.posts(), 1, "1000 回送っても投函は 1 件");

    fake.drain_all();
    assert_eq!(seen.get(), 1000, "値はすべて配送される");
}

#[test]
fn 配送の途中で積まれた値も取りこぼさない() {
    let (fake, tasks) = setup();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let again: Rc<RefCell<Option<naui_core::Sender<i32>>>> = Rc::new(RefCell::new(None));
    let sender = tasks.channel({
        let seen = seen.clone();
        let again = again.clone();
        move |v: i32| {
            seen.borrow_mut().push(v);
            if v == 1 {
                // 配送中の追加。予約を解いた後の空チェックが無いと落ちる。
                let next = again.borrow().as_ref().cloned().expect("送信側");
                next.send(99).expect("送信");
            }
        }
    });
    *again.borrow_mut() = Some(sender.clone());

    sender.send(1).expect("送信");
    fake.drain_all();
    assert_eq!(*seen.borrow(), vec![1, 99]);
}

#[test]
fn 送信側を全部落としても残った値は配送される() {
    let (fake, tasks) = setup();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sender = tasks.channel({
        let seen = seen.clone();
        move |v: &'static str| seen.borrow_mut().push(v)
    });

    sender.send("完了").expect("送信");
    drop(sender);

    fake.drain_all();
    assert_eq!(*seen.borrow(), vec!["完了"], "最後の通知を落とさない");
}

#[test]
fn 配送し切ってから受信側が解放される() {
    let (fake, tasks) = setup();
    let (tracer, dropped) = Tracer::new();
    let sender = tasks.channel(move |_: i32| {
        // ハンドラが目印を抱える。解放されたら落ちる。
        let _ = &tracer;
    });

    sender.send(1).expect("送信");
    drop(sender);
    assert!(!dropped.get(), "配送前に解放しない");

    fake.drain_all();
    assert!(dropped.get(), "配り切ったら解放する");
}

#[test]
fn 終了した後の送信は失敗する() {
    let (fake, tasks) = setup();
    let sender = tasks.channel(|_: i32| {});
    assert!(sender.is_open());

    tasks.shutdown();
    assert!(!sender.is_open());
    assert!(sender.send(1).is_err());
    fake.drain_all();
}

#[test]
fn 終了で受信側が解放される() {
    let (_fake, tasks) = setup();
    let (tracer, dropped) = Tracer::new();
    let _sender = tasks.channel(move |_: i32| {
        let _ = &tracer;
    });

    assert!(!dropped.get());
    tasks.shutdown();
    assert!(dropped.get());
}

#[test]
fn チャネルは互いに独立している() {
    let (fake, tasks) = setup();
    let a = Rc::new(RefCell::new(Vec::new()));
    let b = Rc::new(RefCell::new(Vec::new()));
    let sa = tasks.channel({
        let a = a.clone();
        move |v: i32| a.borrow_mut().push(v)
    });
    let sb = tasks.channel({
        let b = b.clone();
        move |v: i32| b.borrow_mut().push(v)
    });

    sa.send(1).expect("送信");
    sb.send(10).expect("送信");
    sa.send(2).expect("送信");
    fake.drain_all();

    assert_eq!(*a.borrow(), vec![1, 2]);
    assert_eq!(*b.borrow(), vec![10]);
}

// --- タスク ---------------------------------------------------------------

/// poll のたびに自分を起こし、指定回数で終わる future。
struct Countdown {
    left: u32,
    polls: Rc<Cell<u32>>,
}

impl Future for Countdown {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.polls.set(self.polls.get() + 1);
        if self.left == 0 {
            return Poll::Ready(());
        }
        self.left -= 1;
        // poll の中からの自己 wake。予約を解く順番を誤ると、ここで止まる。
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

/// 外から起こされるまで待つ future。
struct Manual {
    waker: Rc<RefCell<Option<Waker>>>,
    ready: Rc<Cell<bool>>,
    polls: Rc<Cell<u32>>,
}

impl Future for Manual {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.polls.set(self.polls.get() + 1);
        if self.ready.get() {
            return Poll::Ready(());
        }
        *self.waker.borrow_mut() = Some(cx.waker().clone());
        Poll::Pending
    }
}

#[test]
fn spawnした処理は完了まで回る() {
    let (fake, tasks) = setup();
    let polls = Rc::new(Cell::new(0));
    let task = tasks.spawn(Countdown {
        left: 3,
        polls: polls.clone(),
    });

    assert_eq!(polls.get(), 0, "その場では poll しない");
    fake.drain_all();
    assert_eq!(polls.get(), 4, "3 回 Pending、4 回目で Ready");
    assert!(task.is_finished());
}

#[test]
fn 自己wakeは取りこぼさない() {
    let (fake, tasks) = setup();
    let polls = Rc::new(Cell::new(0));
    tasks.spawn(Countdown {
        left: 5,
        polls: polls.clone(),
    });

    // 1 ティックにつき 1 回だけ poll され、そのたびに次が予約される。
    for expected in 1..=6 {
        fake.drain();
        assert_eq!(polls.get(), expected);
    }
    assert_eq!(fake.drain(), 0, "終わったら投函されない");
}

#[test]
fn 別スレッドから起こすと進む() {
    let (fake, tasks) = setup();
    let waker = Rc::new(RefCell::new(None));
    let ready = Rc::new(Cell::new(false));
    let polls = Rc::new(Cell::new(0));
    let task = tasks.spawn(Manual {
        waker: waker.clone(),
        ready: ready.clone(),
        polls: polls.clone(),
    });

    fake.drain_all();
    assert_eq!(polls.get(), 1);
    assert!(!task.is_finished());

    let saved = waker.borrow().clone().expect("waker");
    ready.set(true);
    std::thread::spawn(move || saved.wake())
        .join()
        .expect("ワーカー");

    fake.drain_all();
    assert_eq!(polls.get(), 2);
    assert!(task.is_finished());
}

#[test]
fn 多重に起こしても投函は1回にまとまる() {
    let (fake, tasks) = setup();
    let waker = Rc::new(RefCell::new(None));
    let ready = Rc::new(Cell::new(false));
    let polls = Rc::new(Cell::new(0));
    tasks.spawn(Manual {
        waker: waker.clone(),
        ready: ready.clone(),
        polls: polls.clone(),
    });
    fake.drain_all();

    let before = fake.posts();
    let saved = waker.borrow().clone().expect("waker");
    for _ in 0..10 {
        saved.wake_by_ref();
    }
    assert_eq!(fake.posts() - before, 1, "10 回起こしても投函は 1 件");

    fake.drain_all();
    assert_eq!(polls.get(), 2);
}

#[test]
fn 終わった後に古いwakerから起こしても投函しない() {
    let (fake, tasks) = setup();
    let waker = Rc::new(RefCell::new(None));
    let ready = Rc::new(Cell::new(false));
    let polls = Rc::new(Cell::new(0));
    tasks.spawn(Manual {
        waker: waker.clone(),
        ready: ready.clone(),
        polls: polls.clone(),
    });
    fake.drain_all();

    let saved = waker.borrow().clone().expect("waker");
    ready.set(true);
    saved.wake_by_ref();
    fake.drain_all();
    assert_eq!(polls.get(), 2, "ここで完了");

    let before = fake.posts();
    saved.wake_by_ref();
    assert_eq!(fake.posts(), before, "終わった後は投函しない");
}

#[test]
fn cancelした処理はpollされない() {
    let (fake, tasks) = setup();
    let polls = Rc::new(Cell::new(0));
    let task = tasks.spawn(Countdown {
        left: 5,
        polls: polls.clone(),
    });

    fake.drain();
    assert_eq!(polls.get(), 1);

    task.cancel();
    fake.drain_all();
    assert_eq!(polls.get(), 1, "cancel 後は進まない");
}

#[test]
fn cancelすると処理そのものが落ちる() {
    let (fake, tasks) = setup();
    let (tracer, dropped) = Tracer::new();
    let inner = Manual {
        waker: Rc::new(RefCell::new(None)),
        ready: Rc::new(Cell::new(false)),
        polls: Rc::new(Cell::new(0)),
    };
    let task = tasks.spawn(async move {
        let _ = &tracer;
        inner.await;
    });

    fake.drain_all();
    assert!(!dropped.get());

    task.cancel();
    assert!(dropped.get(), "cancel で future ごと落ちる");
}

#[test]
fn 処理の中から自分を止められる() {
    let (fake, tasks) = setup();
    let polls = Rc::new(Cell::new(0));
    let slot: Rc<RefCell<Option<naui_core::Task>>> = Rc::new(RefCell::new(None));
    let task = tasks.spawn(Countdown {
        left: 5,
        polls: polls.clone(),
    });
    *slot.borrow_mut() = Some(task);

    // poll の最中に cancel されるのと同じ形にする。
    let inner = slot.clone();
    let stopper = tasks.spawn(async move {
        if let Some(task) = inner.borrow_mut().take() {
            task.cancel();
        }
    });

    fake.drain_all();
    assert!(stopper.is_finished());
    assert!(polls.get() <= 2, "止めた後は進まない: {}", polls.get());
}

#[test]
fn 処理が落ちても他の処理は生き残る() {
    let (fake, tasks) = setup();
    let polls = Rc::new(Cell::new(0));
    let broken = tasks.spawn(async {
        panic!("わざと落とす");
    });
    tasks.spawn(Countdown {
        left: 2,
        polls: polls.clone(),
    });

    fake.drain_all();
    assert!(broken.is_finished());
    assert_eq!(polls.get(), 3, "隣の処理は最後まで回る");
}

#[test]
fn 終わった処理は保持されない() {
    let (fake, tasks) = setup();
    let (tracer, dropped) = Tracer::new();
    let task = tasks.spawn(async move {
        let _ = &tracer;
    });

    fake.drain_all();
    assert!(task.is_finished());
    assert!(dropped.get(), "終わったら中身ごと落ちる");
}

#[test]
fn 取っ手を捨てても処理は続く() {
    let (fake, tasks) = setup();
    let polls = Rc::new(Cell::new(0));
    drop(tasks.spawn(Countdown {
        left: 3,
        polls: polls.clone(),
    }));

    fake.drain_all();
    assert_eq!(polls.get(), 4, "取っ手を捨てても最後まで回る");
}

#[test]
fn 終了した後のspawnは最初から終わっている() {
    let (fake, tasks) = setup();
    let (tracer, dropped) = Tracer::new();
    tasks.shutdown();

    let task = tasks.spawn(async move {
        let _ = &tracer;
    });
    assert!(task.is_finished());
    assert!(dropped.get(), "回せないので中身は捨てる");
    assert_eq!(fake.drain(), 0);
}

#[test]
fn 終了で残った処理が解放される() {
    let (fake, tasks) = setup();
    let (tracer, dropped) = Tracer::new();
    let inner = Manual {
        waker: Rc::new(RefCell::new(None)),
        ready: Rc::new(Cell::new(false)),
        polls: Rc::new(Cell::new(0)),
    };
    tasks.spawn(async move {
        let _ = &tracer;
        inner.await;
    });
    fake.drain_all();
    assert!(!dropped.get());

    tasks.shutdown();
    assert!(dropped.get());
}
