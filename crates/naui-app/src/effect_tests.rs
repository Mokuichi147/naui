//! Effects, without a window: when they start, how their results come back, and how
//! they are cancelled. A fake event loop stands in for the backend's, with a clock
//! that moves only when told to.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::convert::Infallible;
use std::future::{pending, Future};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{mpsc, Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use naui::Tasks;
use naui_core::{MainThread, Work};

use super::error::tests_support::REPORTED;
use super::{App, Effect, Program, Scoped};

const SECOND: Duration = Duration::from_secs(1);

#[derive(Default)]
struct LoopState {
    now: Duration,
    next: u64,
    /// When each piece of work is due, and in what order it was posted.
    work: Vec<(Duration, u64, Work)>,
}

/// The event loop's waiting room. Other threads post into it, as they do for real.
#[derive(Clone, Default)]
pub(super) struct Loop(Arc<Mutex<LoopState>>);

impl MainThread for Loop {
    fn post(&self, work: Work) -> bool {
        self.post_after(Duration::ZERO, work)
    }

    fn post_after(&self, delay: Duration, work: Work) -> bool {
        let mut state = self.0.lock().expect("lock");
        let (due, order) = (state.now + delay, state.next);
        state.next += 1;
        state.work.push((due, order, work));
        true
    }
}

impl Loop {
    pub(super) fn new() -> (Self, Tasks) {
        let event_loop = Self::default();
        let tasks = Tasks::from_main_thread(Arc::new(event_loop.clone()));
        (event_loop, tasks)
    }

    /// Takes the earliest work due by now. The lock is not held while it runs, since it
    /// posts more.
    fn take_due(&self) -> Option<Work> {
        let mut state = self.0.lock().expect("lock");
        let now = state.now;
        let at = state
            .work
            .iter()
            .enumerate()
            .filter(|(_, (due, ..))| *due <= now)
            .min_by_key(|(_, (due, order, _))| (*due, *order))
            .map(|(at, _)| at)?;
        Some(state.work.remove(at).2)
    }

    /// Runs all that is due, including what that makes due.
    pub(super) fn run(&self) {
        while let Some(work) = self.take_due() {
            work();
        }
    }

    /// Moves the clock on, running what comes due in the order it does.
    fn advance(&self, by: Duration) {
        self.run();
        let end = self.0.lock().expect("lock").now + by;
        loop {
            let next = {
                let mut state = self.0.lock().expect("lock");
                let next = state
                    .work
                    .iter()
                    .map(|(due, ..)| *due)
                    .filter(|due| *due <= end)
                    .min();
                state.now = next.unwrap_or(end);
                next
            };
            if next.is_none() {
                return;
            }
            self.run();
        }
    }

    /// Keeps running for a moment, so that what a thread is just sending arrives.
    fn settle(&self) {
        let deadline = Instant::now() + Duration::from_millis(100);
        while Instant::now() < deadline {
            self.run();
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Runs what arrives from other threads until `done` holds.
    fn run_until(&self, mut done: impl FnMut() -> bool) {
        let start = Instant::now();
        loop {
            self.run();
            if done() {
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

type Log = Rc<RefCell<Vec<String>>>;

fn taken(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.borrow_mut())
}

/// Logs when it is dropped, so that a test sees a future go.
struct Dropped(&'static str, Log);

impl Drop for Dropped {
    fn drop(&mut self) {
        self.1.borrow_mut().push(format!("dropped {}", self.0));
    }
}

/// A model that does what the test gives it for each message, and logs the rest.
struct Script<M: 'static> {
    log: Log,
    on: Box<dyn FnMut(M) -> Effect<M>>,
}

impl<M: std::fmt::Debug + 'static> Program for Script<M> {
    type Message = M;
    type View = ();

    fn update(&mut self, message: M) -> Effect<M> {
        self.log.borrow_mut().push(format!("update {message:?}"));
        (self.on)(message)
    }

    fn render(&self, _: &()) {
        self.log.borrow_mut().push("render".into());
    }
}

fn script<M: std::fmt::Debug + 'static>(
    log: &Log,
    on: impl FnMut(M) -> Effect<M> + 'static,
) -> Script<M> {
    Script {
        log: log.clone(),
        on: Box::new(on),
    }
}

/// A running app whose model is a [`Script`], with the first render out of the way.
fn app<M: std::fmt::Debug + 'static>(
    tasks: &Tasks,
    log: &Log,
    on: impl FnMut(M) -> Effect<M> + 'static,
) -> App<Script<M>> {
    let app = App::with_tasks(tasks, script(log, on), |_| Ok::<_, Infallible>(())).expect("build");
    log.borrow_mut().clear();
    app
}

fn reported() -> Vec<String> {
    REPORTED.with(|reported| std::mem::take(&mut *reported.borrow_mut()))
}

#[derive(Debug, Clone, PartialEq)]
enum Msg {
    Go,
    Done(&'static str),
    Number(i32),
}

// 値としての効果

#[test]
fn an_effect_is_a_value_tests_can_look_at() {
    let none = Effect::<Msg>::none();
    assert!(none.is_none());
    assert_eq!(format!("{none:?}"), "None");
    assert!(Effect::<Msg>::default().is_none());
    assert!(Effect::<Msg>::batch([Effect::none(), Effect::none()]).is_none());

    let timer = Effect::after(SECOND, Msg::Go).key("search");
    assert!(!timer.is_none());
    assert_eq!(timer.keys(), ["search"]);
    assert_eq!(format!("{timer:?}"), r#"Key("search", After(1s))"#);

    let both = Effect::every(SECOND, || Msg::Go).and(Effect::fail("no disk"));
    assert_eq!(both.failures(), ["no disk"]);
    assert_eq!(format!("{both:?}"), r#"[Every(1s), Fail("no disk")]"#);

    let cancel = Effect::<Msg>::cancel("search");
    assert!(!cancel.is_none(), "何かを止めるので、何もしないのではない");
    assert_eq!(cancel.keys(), ["search"]);

    // 包んでも中身は見える。
    let wrapped = Effect::fail("no disk").key("save").map(|_: Msg| 0);
    assert_eq!(wrapped.failures(), ["no disk"]);
    assert_eq!(wrapped.keys(), ["save"]);
}

#[test]
fn an_effect_nobody_runs_does_nothing() {
    let started = Rc::new(Cell::new(false));
    let flag = started.clone();
    let effect = Effect::<Msg>::spawn(move |_| {
        flag.set(true);
        async { Msg::Go }
    });
    drop(effect);
    assert!(!started.get());
}

// いつ始まるか

#[test]
fn effects_start_after_render_once_the_queue_is_empty() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let inner = log.clone();
    let app = app(&tasks, &log, move |message| match message {
        Msg::Go => {
            let log = inner.clone();
            Effect::spawn(move |_| {
                log.borrow_mut().push("started".into());
                async { Msg::Done("task") }
            })
        }
        _ => Effect::none(),
    });
    app.send(Msg::Go);
    assert_eq!(taken(&log), ["update Go", "render", "started"]);
    event_loop.run();
    assert_eq!(taken(&log), [r#"update Done("task")"#, "render"]);
}

#[test]
fn messages_sent_while_building_can_start_work() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let model = script(&log, |message| match message {
        Msg::Go => Effect::after(Duration::ZERO, Msg::Done("started with the app")),
        _ => Effect::none(),
    });
    let _app = App::with_tasks(&tasks, model, |dispatch| {
        dispatch.send(Msg::Go);
        Ok::<_, Infallible>(())
    })
    .expect("build");
    event_loop.run();
    assert_eq!(
        taken(&log),
        [
            "update Go",
            "render",
            r#"update Done("started with the app")"#,
            "render"
        ]
    );
}

#[test]
fn a_panic_in_update_does_not_stop_earlier_effects() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let app = app(&tasks, &log, |message| match message {
        Msg::Go => Effect::after(Duration::ZERO, Msg::Done("kept")),
        Msg::Number(_) => panic!("update panicked (intended)"),
        _ => Effect::none(),
    });
    // Go と Number を同じ周回で処理する。Number の update が panic する。
    app.inner.queue.borrow_mut().push_back(Msg::Go);
    app.send(Msg::Number(1));
    assert_eq!(app.running(), 1);
    event_loop.run();
    assert!(log.borrow().iter().any(|line| line.contains("kept")));
}

#[test]
fn a_panic_while_starting_does_not_stop_the_rest() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let app = app(&tasks, &log, |message| match message {
        Msg::Go => {
            Effect::spawn(|_| -> std::future::Ready<Msg> { panic!("start panicked (intended)") })
                .and(Effect::after(Duration::ZERO, Msg::Done("rest")))
        }
        _ => Effect::none(),
    });
    app.send(Msg::Go);
    assert_eq!(app.running(), 1);
    event_loop.run();
    assert!(log.borrow().iter().any(|line| line.contains("rest")));
}

// 結果の戻り方

#[test]
fn a_task_can_wait_and_its_result_comes_back_as_a_message() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let app = app(&tasks, &log, |message| match message {
        Msg::Go => Effect::spawn(|tasks| async move {
            tasks.sleep(SECOND).await;
            Msg::Done("slept")
        }),
        _ => Effect::none(),
    });
    app.send(Msg::Go);
    assert_eq!(app.running(), 1);
    event_loop.advance(SECOND / 2);
    taken(&log);
    event_loop.advance(SECOND / 2);
    assert_eq!(taken(&log), [r#"update Done("slept")"#, "render"]);
    assert_eq!(app.running(), 0);
}

#[test]
fn timers_send_their_messages_when_due() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let count = Rc::new(Cell::new(0));
    let counted = count.clone();
    let app = app(&tasks, &log, move |message| match message {
        Msg::Go => {
            let count = counted.clone();
            Effect::after(SECOND, Msg::Done("once")).and(Effect::every(SECOND, move || {
                count.set(count.get() + 1);
                Msg::Number(count.get())
            }))
        }
        _ => Effect::none(),
    });
    app.send(Msg::Go);
    taken(&log);
    assert_eq!(app.running(), 2);
    event_loop.advance(SECOND * 3);
    assert_eq!(count.get(), 3);
    let log = taken(&log);
    assert_eq!(
        log.iter().filter(|line| line.contains("once")).count(),
        1,
        "{log:?}"
    );
    assert_eq!(app.running(), 1, "繰り返しのものだけが残る");
}

#[test]
fn a_threads_result_comes_back_as_a_message() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let app = app(&tasks, &log, |message| match message {
        Msg::Go => Effect::thread(|| Msg::Number(6 * 7)),
        _ => Effect::none(),
    });
    app.send(Msg::Go);
    event_loop.run_until(|| log.borrow().iter().any(|line| line.contains("42")));
    event_loop.run_until(|| app.running() == 0);
}

#[test]
fn a_threads_panic_is_reported_as_a_failure() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let app = app(&tasks, &log, |message| match message {
        Msg::Go => Effect::thread(|| panic!("worker panicked (intended)")),
        _ => Effect::none(),
    });
    app.send(Msg::Go);
    event_loop.run_until(|| app.running() == 0);
    assert_eq!(
        reported(),
        ["an effect's thread panicked: worker panicked (intended)"]
    );
}

#[test]
fn a_channel_delivers_everything_sent_in_order_and_then_closes() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let app = app(&tasks, &log, |message| match message {
        Msg::Go => Effect::channel(|sender| {
            std::thread::spawn(move || {
                for number in 1..=3 {
                    sender.send(Msg::Number(number)).expect("send");
                }
            });
        }),
        _ => Effect::none(),
    });
    app.send(Msg::Go);
    assert_eq!(app.running(), 1);
    event_loop.run_until(|| app.running() == 0);
    let updates: Vec<_> = taken(&log)
        .into_iter()
        .filter(|line| line.contains("Number"))
        .collect();
    assert_eq!(
        updates,
        ["update Number(1)", "update Number(2)", "update Number(3)"]
    );
}

#[test]
fn a_result_goes_through_the_queue_without_re_entering_update() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let app = app(&tasks, &log, |message| match message {
        Msg::Go => Effect::after(Duration::ZERO, Msg::Done("first"))
            .and(Effect::after(Duration::ZERO, Msg::Done("second"))),
        _ => Effect::none(),
    });
    app.send(Msg::Go);
    taken(&log);
    event_loop.run();
    assert_eq!(
        taken(&log),
        [
            r#"update Done("first")"#,
            "render",
            r#"update Done("second")"#,
            "render"
        ]
    );
}

#[test]
fn finished_effects_do_not_pile_up() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let app = app(&tasks, &log, |message| match message {
        Msg::Go => Effect::after(Duration::ZERO, Msg::Done("x")),
        _ => Effect::none(),
    });
    for _ in 0..100 {
        app.send(Msg::Go);
        event_loop.run();
    }
    app.send(Msg::Go);
    assert_eq!(app.running(), 1);
    let kept = app.inner.owner.entries();
    assert!(kept <= 2, "the ledger holds {kept} entries");
}

// 失敗

#[test]
fn a_failure_is_reported_after_render() {
    let (_event_loop, tasks) = Loop::new();
    let log = Log::default();
    let app = app(&tasks, &log, |message| match message {
        Msg::Go => Effect::fail(format!("could not save: {}", "disk full")),
        _ => Effect::none(),
    });
    reported();
    app.send(Msg::Go);
    assert_eq!(reported(), ["could not save: disk full"]);
    assert_eq!(taken(&log), ["update Go", "render"]);
}

#[test]
fn a_failed_result_takes_the_same_path() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let app = app(&tasks, &log, |message| match message {
        Msg::Go => Effect::after(Duration::ZERO, Msg::Done("oops")),
        Msg::Done(error) => Effect::fail(error),
        _ => Effect::none(),
    });
    app.send(Msg::Go);
    event_loop.run();
    assert_eq!(reported(), ["oops"]);
}

#[test]
fn without_tasks_a_failure_is_reported_and_work_does_not_start() {
    let log = Log::default();
    let started = Rc::new(Cell::new(false));
    let flag = started.clone();
    let model = script(&log, move |_: Msg| {
        let flag = flag.clone();
        Effect::fail("first").and(Effect::spawn(move |_| {
            flag.set(true);
            async { Msg::Go }
        }))
    });
    let app = App::new(model, |_| Ok::<_, Infallible>(())).expect("build");
    app.send(Msg::Go);
    assert!(!started.get());
    let reported = reported();
    assert_eq!(reported[0], "first");
    assert!(reported[1].contains("App::with_tasks"), "{reported:?}");
}

// キー

#[test]
fn a_timer_under_a_key_is_replaced() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let app = app(&tasks, &log, |message| match message {
        Msg::Number(n) => {
            Effect::after(SECOND, Msg::Done(if n == 1 { "first" } else { "second" })).key("search")
        }
        _ => Effect::none(),
    });
    app.send(Msg::Number(1));
    event_loop.advance(SECOND / 2);
    app.send(Msg::Number(2));
    assert_eq!(app.running(), 1);
    taken(&log);
    event_loop.advance(SECOND * 2);
    assert_eq!(taken(&log), [r#"update Done("second")"#, "render"]);
}

#[test]
fn a_task_under_a_key_is_replaced_and_its_future_dropped() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let inner = log.clone();
    let app = app(&tasks, &log, move |message| match message {
        Msg::Go => {
            let guard = Dropped("task", inner.clone());
            Effect::spawn(move |_| async move {
                let _guard = guard;
                pending::<Msg>().await
            })
            .key("load")
        }
        _ => Effect::none(),
    });
    app.send(Msg::Go);
    event_loop.run();
    taken(&log);
    app.send(Msg::Go);
    assert_eq!(taken(&log), ["update Go", "render", "dropped task"]);
    assert_eq!(app.running(), 1);
}

/// A thread that waits for the test: `release` lets it end with `message`, and
/// `finished` tells that it has sent it.
fn gated_thread(
    message: Msg,
) -> (
    impl FnOnce() -> Msg + Send + 'static,
    mpsc::Sender<()>,
    mpsc::Receiver<()>,
) {
    let (release, wait) = mpsc::channel();
    let (finish, finished) = mpsc::channel();
    let work = move || {
        wait.recv().expect("released");
        finish.send(()).expect("finished");
        message
    };
    (work, release, finished)
}

#[test]
fn a_threads_result_under_a_key_is_discarded_when_replaced() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let (slow, release, finished) = gated_thread(Msg::Done("overtaken"));
    let slow = RefCell::new(Some(slow));
    let app = app(&tasks, &log, move |message| match message {
        Msg::Go => match slow.borrow_mut().take() {
            Some(work) => Effect::thread(work).key("search"),
            None => Effect::thread(|| Msg::Done("latest")).key("search"),
        },
        _ => Effect::none(),
    });
    app.send(Msg::Go);
    app.send(Msg::Go);
    release.send(()).expect("release");
    finished.recv().expect("finished");
    event_loop.run_until(|| log.borrow().iter().any(|line| line.contains("latest")));
    // 古いほうのスレッドが送った値は、届いても捨てられる。
    event_loop.settle();
    assert!(
        !log.borrow().iter().any(|line| line.contains("overtaken")),
        "{:?}",
        log.borrow()
    );
}

#[test]
fn cancel_stops_what_runs_under_the_key() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let app = app(&tasks, &log, |message| match message {
        Msg::Go => Effect::every(SECOND, || Msg::Done("tick")).key("clock"),
        Msg::Number(_) => Effect::cancel("clock"),
        _ => Effect::none(),
    });
    app.send(Msg::Go);
    event_loop.advance(SECOND);
    assert_eq!(app.running(), 1);
    app.send(Msg::Number(0));
    assert_eq!(app.running(), 0);
    taken(&log);
    event_loop.advance(SECOND * 5);
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
}

#[test]
fn a_batch_under_one_key_runs_together_and_goes_together() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let app = app(&tasks, &log, |message| match message {
        Msg::Go => Effect::batch([
            Effect::after(SECOND, Msg::Done("a")),
            Effect::after(SECOND, Msg::Done("b")),
        ])
        .key("pair"),
        Msg::Number(_) => Effect::cancel("pair"),
        _ => Effect::none(),
    });
    app.send(Msg::Go);
    assert_eq!(app.running(), 2);
    app.send(Msg::Number(0));
    assert_eq!(app.running(), 0);
    event_loop.advance(SECOND * 2);
}

#[test]
fn two_effects_under_one_key_in_one_update_replace_each_other() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let app = app(&tasks, &log, |message| match message {
        Msg::Go => Effect::after(SECOND, Msg::Done("a"))
            .key("k")
            .and(Effect::after(SECOND, Msg::Done("b")).key("k")),
        _ => Effect::none(),
    });
    app.send(Msg::Go);
    assert_eq!(app.running(), 1);
    taken(&log);
    event_loop.advance(SECOND);
    assert_eq!(taken(&log), [r#"update Done("b")"#, "render"]);
}

#[test]
fn a_task_can_be_replaced_by_the_update_that_handles_its_result() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let app = app(&tasks, &log, |message| match message {
        Msg::Go => Effect::spawn(|_| async { Msg::Done("first") }).key("k"),
        // 結果を処理している間、タスクはまだ poll の中にいる。
        Msg::Done("first") => Effect::spawn(|_| async { Msg::Done("second") }).key("k"),
        _ => Effect::none(),
    });
    app.send(Msg::Go);
    event_loop.run();
    assert!(log.borrow().iter().any(|line| line.contains("second")));
    assert_eq!(app.running(), 0);
}

// 親子

#[derive(Debug)]
enum ParentMsg {
    Child(u32, Msg),
    Remove(u32),
    Later,
}

struct Parent {
    children: HashMap<u32, Scoped<Script<Msg>>>,
    log: Log,
}

impl Program for Parent {
    type Message = ParentMsg;
    type View = ();

    fn update(&mut self, message: ParentMsg) -> Effect<ParentMsg> {
        self.log.borrow_mut().push(format!("parent {message:?}"));
        match message {
            ParentMsg::Child(id, message) => match self.children.get_mut(&id) {
                Some(child) => child
                    .update(message)
                    .map(move |message| ParentMsg::Child(id, message)),
                None => Effect::none(),
            },
            ParentMsg::Remove(id) => {
                self.children.remove(&id);
                Effect::none()
            }
            ParentMsg::Later => Effect::after(SECOND, ParentMsg::Later).key("search"),
        }
    }

    fn render(&self, _: &()) {}
}

fn parent(
    tasks: &Tasks,
    log: &Log,
    on: impl Fn(Msg) -> Effect<Msg> + Clone + 'static,
) -> App<Parent> {
    let children = [1, 2]
        .into_iter()
        .map(|id| (id, Scoped::new(script(log, on.clone()))))
        .collect();
    let model = Parent {
        children,
        log: log.clone(),
    };
    let app = App::with_tasks(tasks, model, |_| Ok::<_, Infallible>(())).expect("build");
    log.borrow_mut().clear();
    app
}

fn child_running(app: &App<Parent>, id: u32) -> usize {
    app.model().children[&id].running()
}

#[test]
fn a_childs_effect_is_mapped_into_the_parents_message() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let app = parent(&tasks, &log, |message| match message {
        Msg::Go => Effect::after(SECOND, Msg::Done("mapped")),
        _ => Effect::none(),
    });
    app.send(ParentMsg::Child(1, Msg::Go));
    assert_eq!(child_running(&app, 1), 1);
    assert_eq!(child_running(&app, 2), 0);
    assert_eq!(app.running(), 0, "親の持ち物ではない");
    taken(&log);
    event_loop.advance(SECOND);
    assert_eq!(
        taken(&log),
        [
            r#"parent Child(1, Done("mapped"))"#,
            r#"update Done("mapped")"#
        ]
    );
}

#[test]
fn dropping_a_wrapper_cancels_its_tasks_and_timers() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let inner = log.clone();
    let app = parent(&tasks, &log, move |message| match message {
        Msg::Go => {
            let guard = Dropped("task", inner.clone());
            Effect::spawn(move |_| async move {
                let _guard = guard;
                pending::<Msg>().await
            })
            .and(Effect::every(SECOND, || Msg::Done("tick")))
        }
        _ => Effect::none(),
    });
    app.send(ParentMsg::Child(1, Msg::Go));
    event_loop.run();
    assert_eq!(child_running(&app, 1), 2);
    taken(&log);
    app.send(ParentMsg::Remove(1));
    assert_eq!(taken(&log), ["parent Remove(1)", "dropped task"]);
    event_loop.advance(SECOND * 5);
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
}

#[test]
fn dropping_a_wrapper_discards_the_result_of_its_thread() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let (work, release, finished) = gated_thread(Msg::Done("too late"));
    let work = Rc::new(RefCell::new(Some(work)));
    let app = parent(&tasks, &log, move |message| match message {
        Msg::Go => {
            let work = work.borrow_mut().take().expect("once");
            Effect::thread(work)
        }
        _ => Effect::none(),
    });
    app.send(ParentMsg::Child(1, Msg::Go));
    app.send(ParentMsg::Remove(1));
    taken(&log);
    release.send(()).expect("release");
    finished.recv().expect("finished");
    event_loop.settle();
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
}

#[test]
fn an_effect_not_started_yet_does_not_start_once_its_wrapper_is_gone() {
    let (_event_loop, tasks) = Loop::new();
    let log = Log::default();
    let started = Rc::new(Cell::new(false));
    let flag = started.clone();
    let app = parent(&tasks, &log, move |message| match message {
        Msg::Go => {
            let flag = flag.clone();
            Effect::spawn(move |_| {
                flag.set(true);
                async { Msg::Done("x") }
            })
            .and(Effect::fail("still reported"))
        }
        _ => Effect::none(),
    });
    reported();
    // 同じ周回で、効果を頼んだ子がいなくなる。効果が始まるのは render のあと。
    app.inner
        .queue
        .borrow_mut()
        .push_back(ParentMsg::Child(1, Msg::Go));
    app.send(ParentMsg::Remove(1));
    assert!(!started.get());
    assert_eq!(reported(), ["still reported"]);
}

#[test]
fn keys_belong_to_a_model() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let app = parent(&tasks, &log, |message| match message {
        Msg::Go => Effect::after(SECOND, Msg::Done("done")).key("search"),
        _ => Effect::none(),
    });
    app.send(ParentMsg::Later);
    app.send(ParentMsg::Child(1, Msg::Go));
    app.send(ParentMsg::Child(2, Msg::Go));
    assert_eq!(child_running(&app, 1), 1);
    assert_eq!(child_running(&app, 2), 1);
    assert_eq!(app.running(), 1);
    // 片方の子で置き換えても、もう片方と親は残る。
    app.send(ParentMsg::Child(1, Msg::Go));
    assert_eq!(child_running(&app, 1), 1);
    assert_eq!(child_running(&app, 2), 1);
    assert_eq!(app.running(), 1);
    taken(&log);
    event_loop.advance(SECOND);
    let log = taken(&log);
    assert_eq!(log.iter().filter(|line| line.contains("done")).count(), 4);
}

#[test]
fn a_parents_key_over_a_childs_effect_replaces_it() {
    let (_event_loop, tasks) = Loop::new();
    let log = Log::default();

    struct Over {
        child: Scoped<Script<Msg>>,
    }
    impl Program for Over {
        type Message = Msg;
        type View = ();
        fn update(&mut self, message: Msg) -> Effect<Msg> {
            match message {
                Msg::Go => self.child.update(Msg::Go).key("outer"),
                _ => Effect::cancel("outer"),
            }
        }
        fn render(&self, _: &()) {}
    }

    let child = Scoped::new(script(&log, |message| match message {
        Msg::Go => Effect::every(SECOND, || Msg::Done("tick")),
        _ => Effect::none(),
    }));
    let app = App::with_tasks(&tasks, Over { child }, |_| Ok::<_, Infallible>(())).expect("build");
    app.send(Msg::Go);
    assert_eq!(app.model().child.running(), 1);
    assert_eq!(app.running(), 0, "持ち主は子のまま");
    app.send(Msg::Go);
    assert_eq!(app.model().child.running(), 1, "前のものは置き換えられた");
    app.send(Msg::Number(0));
    assert_eq!(app.model().child.running(), 0);
}

#[test]
fn an_inner_wrapper_goes_with_the_outer_one() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();

    struct Middle {
        inner: Scoped<Script<Msg>>,
    }
    impl Program for Middle {
        type Message = Msg;
        type View = ();
        fn update(&mut self, message: Msg) -> Effect<Msg> {
            self.inner.update(message)
        }
        fn render(&self, _: &()) {}
    }
    struct Outer {
        middle: Option<Scoped<Middle>>,
    }
    impl Program for Outer {
        type Message = Msg;
        type View = ();
        fn update(&mut self, message: Msg) -> Effect<Msg> {
            match message {
                Msg::Number(_) => {
                    self.middle = None;
                    Effect::none()
                }
                message => match &mut self.middle {
                    Some(middle) => middle.update(message),
                    None => Effect::none(),
                },
            }
        }
        fn render(&self, _: &()) {}
    }

    let inner = Scoped::new(script(&log, |message| match message {
        Msg::Go => Effect::every(SECOND, || Msg::Done("tick")),
        _ => Effect::none(),
    }));
    let middle = Some(Scoped::new(Middle { inner }));
    let app =
        App::with_tasks(&tasks, Outer { middle }, |_| Ok::<_, Infallible>(())).expect("build");
    app.send(Msg::Go);
    {
        let model = app.model();
        let middle = model.middle.as_ref().expect("middle");
        assert_eq!(middle.inner.running(), 1, "一番内側のものが持つ");
        assert_eq!(middle.running(), 0);
    }
    app.send(Msg::Number(0));
    taken(&log);
    event_loop.advance(SECOND * 3);
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
}

#[test]
fn dropping_the_app_cancels_its_own_effects() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let inner = log.clone();
    let app = app(&tasks, &log, move |message| match message {
        Msg::Go => {
            let guard = Dropped("task", inner.clone());
            Effect::spawn(move |_| async move {
                let _guard = guard;
                pending::<Msg>().await
            })
            .and(Effect::every(SECOND, || Msg::Done("tick")))
        }
        _ => Effect::none(),
    });
    app.send(Msg::Go);
    event_loop.run();
    taken(&log);
    drop(app);
    assert_eq!(taken(&log), ["dropped task"]);
    event_loop.advance(SECOND * 3);
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
}

#[test]
fn a_scoped_model_reads_and_changes_like_the_child() {
    struct Counter(i32);
    impl Program for Counter {
        type Message = i32;
        type View = ();
        fn update(&mut self, by: i32) -> Effect<i32> {
            self.0 += by;
            Effect::none()
        }
        fn render(&self, _: &()) {}
    }
    let mut counter = Scoped::new(Counter(1));
    assert!(counter.update(2).is_none());
    assert_eq!(counter.0, 3);
    counter.0 = 10;
    assert_eq!(counter.0, 10);
    assert_eq!(counter.running(), 0);
}

#[test]
fn a_waker_held_after_cancelling_does_nothing() {
    struct Hold(Rc<RefCell<Option<Waker>>>);
    impl Future for Hold {
        type Output = Msg;
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Msg> {
            *self.0.borrow_mut() = Some(cx.waker().clone());
            Poll::Pending
        }
    }
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let waker = Rc::new(RefCell::new(None));
    let held = waker.clone();
    let app = app(&tasks, &log, move |message| match message {
        Msg::Go => {
            let held = held.clone();
            Effect::spawn(move |_| Hold(held)).key("k")
        }
        _ => Effect::cancel("k"),
    });
    app.send(Msg::Go);
    event_loop.run();
    let waker = waker.borrow_mut().take().expect("polled");
    app.send(Msg::Number(0));
    taken(&log);
    waker.wake();
    event_loop.run();
    assert!(log.borrow().is_empty());
}
