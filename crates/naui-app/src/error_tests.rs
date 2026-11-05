//! The handler, without a window: what it is told, and what its answer does. Panics that
//! cannot be caught (`panic = "abort"`, wasm) need a process of their own, and are
//! checked by hand; `cargo test` always unwinds.

use std::cell::{Cell, RefCell};
use std::convert::Infallible;
use std::fmt;
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::time::{Duration, Instant};

use naui::Tasks;

use super::component::Keyed;
use super::effect_tests::Loop;
use super::error::tests_support::REPORTED;
use super::{App, Decision, Dispatch, Effect, Program, Report, ReportKind};

#[derive(Debug, Clone, PartialEq)]
enum Msg {
    Plain,
    Named(&'static str),
    /// What the handler sent.
    FromHandler,
}

type Log = Rc<RefCell<Vec<String>>>;

fn taken(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.borrow_mut())
}

/// A model that does what the test gives it, and logs the rest.
struct Script {
    log: Log,
    on_update: Box<dyn FnMut(Msg) -> Effect<Msg>>,
    on_render: Rc<dyn Fn()>,
}

impl Program for Script {
    type Message = Msg;
    type View = ();

    fn update(&mut self, message: Msg) -> Effect<Msg> {
        self.log.borrow_mut().push(format!("update {message:?}"));
        (self.on_update)(message)
    }

    fn render(&self, _: &()) {
        self.log.borrow_mut().push("render".into());
        (self.on_render)();
    }
}

/// What the handler saw, in order.
type Seen = Rc<RefCell<Vec<Report>>>;

/// A running app, its first render over, whose handler records and answers `answer`.
fn app_answering(
    tasks: Option<&Tasks>,
    answer: Decision,
    on_update: impl FnMut(Msg) -> Effect<Msg> + 'static,
) -> (App<Script>, Log, Seen) {
    let log = Log::default();
    let app = start(tasks, &log, on_update, Rc::new(|| {}));
    let seen = Seen::default();
    let recorded = seen.clone();
    app.set_error_handler(move |report, _| {
        recorded.borrow_mut().push(report.clone());
        answer
    });
    taken(&log);
    (app, log, seen)
}

fn start(
    tasks: Option<&Tasks>,
    log: &Log,
    on_update: impl FnMut(Msg) -> Effect<Msg> + 'static,
    on_render: Rc<dyn Fn()>,
) -> App<Script> {
    let model = Script {
        log: log.clone(),
        on_update: Box::new(on_update),
        on_render,
    };
    let build = |_: &Dispatch<Msg>| Ok::<_, Infallible>(());
    match tasks {
        Some(tasks) => App::with_tasks(tasks, model, build),
        None => App::new(model, build),
    }
    .expect("build")
}

fn panics_on_named(message: Msg) -> Effect<Msg> {
    if let Msg::Named(name) = message {
        panic!("{name} panicked (intended)");
    }
    Effect::none()
}

/// What went to standard error on this thread, since the last call.
fn to_standard_error() -> Vec<String> {
    REPORTED.with(|reported| std::mem::take(&mut *reported.borrow_mut()))
}

// 失敗

#[test]
fn a_failure_carries_the_message_update_was_given() {
    let (app, _, seen) = app_answering(None, Decision::GoOn, |message| match message {
        Msg::Named(name) => Effect::fail(format!("{name} failed")),
        _ => Effect::none(),
    });
    app.send(Msg::Named("save"));
    let seen = seen.borrow();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].kind(), ReportKind::Failure);
    assert_eq!(seen[0].text(), "save failed");
    assert_eq!(seen[0].message(), Some(r#"Named("save")"#));
    assert_eq!(seen[0].location(), None);
    assert_eq!(
        seen[0].to_string(),
        r#"save failed (while handling Named("save"))"#
    );
    assert!(to_standard_error().is_empty(), "ハンドラへ渡ったら出さない");
}

#[test]
fn a_failure_carries_its_own_message_when_several_were_handled() {
    let (app, _, seen) = app_answering(None, Decision::GoOn, |message| match message {
        Msg::Named(name) => Effect::fail(name),
        _ => Effect::none(),
    });
    // 同じ周回で処理する。効果が始まるのは、すべてが終わった render のあと。
    app.inner.queue.borrow_mut().push_back(Msg::Named("first"));
    app.inner.queue.borrow_mut().push_back(Msg::Plain);
    app.send(Msg::Named("second"));
    let seen = seen.borrow();
    assert_eq!(seen[0].text(), "first");
    assert_eq!(seen[0].message(), Some(r#"Named("first")"#));
    assert_eq!(seen[1].text(), "second");
    assert_eq!(seen[1].message(), Some(r#"Named("second")"#));
}

#[test]
fn a_failed_thread_arrives_without_a_message() {
    let (event_loop, tasks) = Loop::new();
    let (app, _, seen) = app_answering(Some(&tasks), Decision::GoOn, |message| match message {
        Msg::Plain => Effect::thread(|| panic!("worker panicked (intended)")),
        _ => Effect::none(),
    });
    app.send(Msg::Plain);
    let start = Instant::now();
    while seen.borrow().is_empty() {
        event_loop.run();
        assert!(start.elapsed() < Duration::from_secs(10), "timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
    // 別スレッドの panic は hook からは伝わらず、失敗として 1 回だけ伝わる。
    let seen = seen.borrow();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].kind(), ReportKind::Failure);
    assert_eq!(
        seen[0].text(),
        "an effect's thread panicked: worker panicked (intended)"
    );
    assert_eq!(seen[0].message(), None);
}

#[test]
fn without_a_handler_a_failure_goes_to_standard_error() {
    let log = Log::default();
    let app = start(None, &log, |_| Effect::fail("oops"), Rc::new(|| {}));
    app.send(Msg::Plain);
    assert_eq!(to_standard_error(), ["oops"]);
}

#[test]
fn a_handler_goes_with_its_app() {
    let (app, _, seen) = app_answering(None, Decision::GoOn, |_| Effect::none());
    drop(app);
    let log = Log::default();
    let other = start(None, &log, |_| Effect::fail("oops"), Rc::new(|| {}));
    other.send(Msg::Plain);
    assert!(seen.borrow().is_empty());
    assert_eq!(to_standard_error(), ["oops"]);
}

#[test]
fn a_new_handler_replaces_the_old_one() {
    let (app, _, seen) = app_answering(None, Decision::GoOn, |_| Effect::fail("oops"));
    let newer = Seen::default();
    let recorded = newer.clone();
    app.set_error_handler(move |report, _| {
        recorded.borrow_mut().push(report.clone());
        Decision::GoOn
    });
    app.send(Msg::Plain);
    assert!(seen.borrow().is_empty());
    assert_eq!(newer.borrow().len(), 1);
}

// キューの中の panic

#[test]
fn a_panic_in_update_is_caught_and_carries_the_message() {
    let (app, log, seen) = app_answering(None, Decision::GoOn, panics_on_named);
    app.send(Msg::Named("save"));
    {
        let seen = seen.borrow();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].kind(), ReportKind::Panic);
        assert_eq!(seen[0].text(), "save panicked (intended)");
        assert_eq!(seen[0].message(), Some(r#"Named("save")"#));
        let location = seen[0].location().expect("a location");
        assert!(location.contains("error_tests.rs"), "{location}");
        let text = seen[0].to_string();
        assert!(text.starts_with("panic at "), "{text}");
        assert!(
            text.ends_with(r#": save panicked (intended) (while handling Named("save"))"#),
            "{text}"
        );
    }
    // 続けるなら、いつものように render してから次のメッセージへ。
    assert_eq!(taken(&log), [r#"update Named("save")"#, "render"]);
    app.send(Msg::Plain);
    assert_eq!(taken(&log), ["update Plain", "render"]);
    assert!(!app.is_stopped());
}

#[test]
fn a_panic_in_render_carries_the_last_message() {
    let log = Log::default();
    let explode = Rc::new(Cell::new(false));
    let exploding = explode.clone();
    let app = start(
        None,
        &log,
        |_| Effect::none(),
        Rc::new(move || {
            if exploding.get() {
                panic!("render panicked (intended)");
            }
        }),
    );
    let seen = Seen::default();
    let recorded = seen.clone();
    app.set_error_handler(move |report, _| {
        recorded.borrow_mut().push(report.clone());
        Decision::GoOn
    });
    explode.set(true);
    app.send(Msg::Named("draw"));
    assert_eq!(seen.borrow()[0].text(), "render panicked (intended)");
    assert_eq!(seen.borrow()[0].message(), Some(r#"Named("draw")"#));
    // render が続けて panic しても、次に変わるまでは描き直さない。
    assert_eq!(seen.borrow().len(), 1);
    explode.set(false);
    app.send(Msg::Plain);
    assert_eq!(seen.borrow().len(), 1);
}

#[test]
fn a_panic_while_an_effect_starts_carries_its_message() {
    let (event_loop, tasks) = Loop::new();
    let (app, _, seen) = app_answering(Some(&tasks), Decision::GoOn, |message| match message {
        Msg::Named(_) => {
            Effect::spawn(|_| -> std::future::Ready<Msg> { panic!("start panicked (intended)") })
        }
        _ => Effect::none(),
    });
    app.send(Msg::Named("load"));
    event_loop.run();
    let seen = seen.borrow();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].text(), "start panicked (intended)");
    assert_eq!(seen[0].message(), Some(r#"Named("load")"#));
}

#[test]
fn a_panic_resumed_without_the_hook_is_still_told() {
    let (app, _, seen) = app_answering(None, Decision::GoOn, |_| {
        resume_unwind(Box::new("resumed (intended)"))
    });
    app.send(Msg::Plain);
    let seen = seen.borrow();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].text(), "resumed (intended)");
    assert_eq!(seen[0].location(), None);
    assert_eq!(seen[0].message(), Some("Plain"));
}

#[test]
fn every_panic_of_a_step_is_told() {
    // 子が 2 つ panic しても、外へ続くのは最初の 1 つだけ。hook は両方を見ている。
    let (app, _, seen) = app_answering(None, Decision::GoOn, |_| {
        let _ = catch_unwind(|| panic!("first (intended)"));
        panic!("second (intended)")
    });
    app.send(Msg::Plain);
    let seen = seen.borrow();
    let texts: Vec<_> = seen.iter().map(Report::text).collect();
    assert_eq!(texts, ["first (intended)", "second (intended)"]);
}

#[test]
fn a_panic_the_app_catches_itself_is_told_all_the_same() {
    let (app, log, seen) = app_answering(None, Decision::GoOn, |_| {
        let _ = catch_unwind(|| panic!("caught inside (intended)"));
        Effect::none()
    });
    app.send(Msg::Plain);
    assert_eq!(seen.borrow()[0].text(), "caught inside (intended)");
    assert_eq!(taken(&log), ["update Plain", "render"]);
}

#[test]
fn without_a_handler_a_panic_does_not_stop_the_queue() {
    let log = Log::default();
    let app = start(None, &log, panics_on_named, Rc::new(|| {}));
    taken(&log);
    app.send(Msg::Named("save"));
    app.send(Msg::Plain);
    assert!(!app.is_stopped());
    assert_eq!(
        taken(&log),
        [
            r#"update Named("save")"#,
            "render",
            "update Plain",
            "render"
        ]
    );
    assert!(to_standard_error().is_empty(), "前の hook に任せる");
}

// 止める

#[test]
fn stopping_ends_the_queue_and_quits() {
    let (event_loop, tasks) = Loop::new();
    let (app, log, seen) = app_answering(Some(&tasks), Decision::Stop, panics_on_named);
    let quit = Rc::new(Cell::new(false));
    let quitting = quit.clone();
    *app.inner.quit.borrow_mut() = Some(Box::new(move || quitting.set(true)));
    // 同じ周回の残りも処理しない。
    app.inner.queue.borrow_mut().push_back(Msg::Named("save"));
    app.send(Msg::Plain);
    assert!(app.is_stopped());
    assert_eq!(seen.borrow().len(), 1);
    assert_eq!(taken(&log), [r#"update Named("save")"#], "render もしない");
    assert_eq!(app.pending(), 0);
    assert!(!quit.get(), "hook の中かもしれないので、ここでは閉じない");
    event_loop.run();
    assert!(quit.get());
    // 止まったあとに送ったものは捨てる。
    app.send(Msg::Plain);
    assert!(taken(&log).is_empty());
    assert_eq!(app.pending(), 0);
}

#[test]
fn stopping_cancels_the_apps_effects() {
    let (event_loop, tasks) = Loop::new();
    let (app, log, _) = app_answering(Some(&tasks), Decision::Stop, |message| match message {
        // 時計を進めないので、取り消されなければ動き続けている。
        Msg::Plain => Effect::after(Duration::from_secs(1), Msg::FromHandler),
        Msg::Named(_) => panic!("update panicked (intended)"),
        Msg::FromHandler => Effect::none(),
    });
    app.send(Msg::Plain);
    assert_eq!(app.running(), 1);
    app.send(Msg::Named("save"));
    event_loop.run();
    assert_eq!(app.running(), 0);
    assert!(!taken(&log).contains(&"update FromHandler".to_owned()));
}

#[test]
fn stopping_closes_the_channels_of_the_apps_effects() {
    let (event_loop, tasks) = Loop::new();
    let saved = Rc::new(RefCell::new(None));
    let slot = saved.clone();
    let (app, _, _) = app_answering(Some(&tasks), Decision::Stop, move |message| match message {
        Msg::Plain => {
            let slot = slot.clone();
            Effect::channel(move |sender| *slot.borrow_mut() = Some(sender))
        }
        Msg::Named(_) => panic!("update panicked (intended)"),
        Msg::FromHandler => Effect::none(),
    });
    app.send(Msg::Plain);
    let sender: naui::Sender<Msg> = saved.borrow_mut().take().expect("started");
    assert!(sender.is_open());
    app.send(Msg::Named("save"));
    event_loop.run();
    assert!(!sender.is_open(), "別スレッドの処理が止まったと分かる");
}

#[test]
fn a_failure_can_stop_the_app_too() {
    let (app, _, seen) = app_answering(None, Decision::Stop, |_| Effect::fail("fatal"));
    app.send(Msg::Plain);
    assert!(app.is_stopped());
    assert_eq!(seen.borrow().len(), 1);
}

#[test]
fn failures_still_waiting_when_the_app_stops_are_reported() {
    let (app, _, seen) = app_answering(None, Decision::Stop, |message| match message {
        Msg::Plain => Effect::fail("happened before"),
        _ => panic!("update panicked (intended)"),
    });
    app.inner.queue.borrow_mut().push_back(Msg::Plain);
    app.send(Msg::Named("save"));
    let seen = seen.borrow();
    let texts: Vec<_> = seen.iter().map(Report::text).collect();
    assert_eq!(
        texts,
        ["update panicked (intended)", "happened before"],
        "始まらなくても、起きた失敗は伝える"
    );
    assert_eq!(seen[1].message(), Some("Plain"));
}

/// An effect that notes it has started, and asks for nothing more.
fn noting(started: &Log, name: &'static str) -> Effect<Msg> {
    let started = started.clone();
    Effect::channel(move |_: naui::Sender<Msg>| started.borrow_mut().push(name.to_owned()))
}

#[test]
fn stopping_on_a_failure_starts_nothing_more_of_the_batch() {
    let (event_loop, tasks) = Loop::new();
    let started = Log::default();
    let noted = started.clone();
    let (app, _, seen) = app_answering(Some(&tasks), Decision::Stop, move |_| {
        noting(&noted, "before")
            .and(Effect::fail("fatal"))
            .and(noting(&noted, "after"))
            .and(noting(&noted, "keyed").key("search"))
            .and(Effect::fail("also happened"))
    });
    app.send(Msg::Plain);
    assert!(app.is_stopped());
    assert_eq!(taken(&started), ["before"], "fail の後ろは始まらない");
    let texts: Vec<_> = seen.borrow().iter().map(|r| r.text().to_owned()).collect();
    assert_eq!(texts, ["fatal", "also happened"], "起きた失敗は伝える");
    event_loop.run();
    assert_eq!(app.running(), 0);
}

#[test]
fn stopping_on_a_panicking_effect_starts_nothing_more_of_the_batch() {
    let (event_loop, tasks) = Loop::new();
    let started = Log::default();
    let noted = started.clone();
    let (app, _, seen) = app_answering(Some(&tasks), Decision::Stop, move |_| {
        Effect::spawn(|_| -> std::future::Ready<Msg> { panic!("start panicked (intended)") })
            .and(noting(&noted, "after"))
    });
    app.send(Msg::Plain);
    assert!(app.is_stopped());
    assert!(taken(&started).is_empty(), "panic の後ろは始まらない");
    assert_eq!(seen.borrow().len(), 1);
    event_loop.run();
}

#[test]
fn going_on_starts_the_rest_of_the_batch() {
    let (event_loop, tasks) = Loop::new();
    let started = Log::default();
    let noted = started.clone();
    let (app, _, seen) = app_answering(Some(&tasks), Decision::GoOn, move |_| {
        Effect::fail("not fatal")
            .and(noting(&noted, "after the failure"))
            .and(Effect::spawn(|_| -> std::future::Ready<Msg> {
                panic!("start panicked (intended)")
            }))
            .and(noting(&noted, "after the panic"))
    });
    app.send(Msg::Plain);
    assert_eq!(taken(&started), ["after the failure", "after the panic"]);
    let texts: Vec<_> = seen.borrow().iter().map(|r| r.text().to_owned()).collect();
    assert_eq!(texts, ["not fatal", "start panicked (intended)"]);
    event_loop.run();
}

#[test]
fn work_after_stopping_does_not_complain_about_missing_tasks() {
    // Tasks がなければ Work は失敗として報告されるが、始めないものは失敗でもない。
    let (app, _, seen) = app_answering(None, Decision::Stop, |_| {
        Effect::fail("fatal").and(Effect::after(Duration::ZERO, Msg::Plain))
    });
    app.send(Msg::Plain);
    let texts: Vec<_> = seen.borrow().iter().map(|r| r.text().to_owned()).collect();
    assert_eq!(texts, ["fatal"]);
}

// キューの外の panic

#[test]
fn a_panic_outside_the_queue_is_told_without_a_message() {
    let (app, _, seen) = app_answering(None, Decision::GoOn, |_| Effect::none());
    app.send(Msg::Named("handled before"));
    let outcome = catch_unwind(|| panic!("callback panicked (intended)"));
    assert!(outcome.is_err());
    let seen = seen.borrow();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].kind(), ReportKind::Panic);
    assert_eq!(seen[0].text(), "callback panicked (intended)");
    assert_eq!(seen[0].message(), None);
    assert!(seen[0].location().is_some());
}

#[test]
fn a_panic_outside_the_queue_can_stop_the_app() {
    let (event_loop, tasks) = Loop::new();
    let (app, log, _) = app_answering(Some(&tasks), Decision::Stop, |_| Effect::none());
    let quit = Rc::new(Cell::new(false));
    let quitting = quit.clone();
    *app.inner.quit.borrow_mut() = Some(Box::new(move || quitting.set(true)));
    let _ = catch_unwind(|| panic!("callback panicked (intended)"));
    assert!(app.is_stopped());
    assert!(!quit.get());
    event_loop.run();
    assert!(quit.get());
    app.send(Msg::Plain);
    assert!(taken(&log).is_empty());
}

#[test]
fn a_panic_on_another_thread_does_not_reach_the_handler() {
    let (_app, _, seen) = app_answering(None, Decision::GoOn, |_| Effect::none());
    let thread = std::thread::spawn(|| panic!("thread panicked (intended)"));
    assert!(thread.join().is_err());
    assert!(seen.borrow().is_empty());
}

// ハンドラからできること

#[test]
fn what_the_handler_sends_from_inside_the_queue_is_handled_next() {
    let log = Log::default();
    let app = start(None, &log, panics_on_named, Rc::new(|| {}));
    app.set_error_handler(|report, dispatch| {
        if report.kind() == ReportKind::Panic {
            dispatch.send(Msg::FromHandler);
        }
        Decision::GoOn
    });
    taken(&log);
    app.send(Msg::Named("save"));
    assert_eq!(
        taken(&log),
        [r#"update Named("save")"#, "update FromHandler", "render"]
    );
}

#[test]
fn what_the_handler_sends_from_the_hook_waits_for_the_event_loop() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let app = start(Some(&tasks), &log, |_| Effect::none(), Rc::new(|| {}));
    app.set_error_handler(|_, dispatch| {
        dispatch.send(Msg::FromHandler);
        Decision::GoOn
    });
    taken(&log);
    let _ = catch_unwind(|| panic!("callback panicked (intended)"));
    assert!(taken(&log).is_empty(), "panic の最中には update を呼ばない");
    assert_eq!(app.pending(), 1);
    event_loop.run();
    assert_eq!(taken(&log), ["update FromHandler", "render"]);
}

#[test]
fn what_the_handler_sends_about_a_failure_outside_the_queue_waits_too() {
    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let app = start(
        Some(&tasks),
        &log,
        |message| match message {
            Msg::Plain => Effect::thread(|| panic!("worker panicked (intended)")),
            _ => Effect::none(),
        },
        Rc::new(|| {}),
    );
    app.set_error_handler(|_, dispatch| {
        dispatch.send(Msg::FromHandler);
        Decision::GoOn
    });
    app.send(Msg::Plain);
    taken(&log);
    let start = Instant::now();
    while !log.borrow().contains(&"update FromHandler".to_owned()) {
        event_loop.run();
        assert!(start.elapsed() < Duration::from_secs(10), "timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn a_panicking_handler_does_not_stop_the_queue() {
    let log = Log::default();
    let app = start(None, &log, panics_on_named, Rc::new(|| {}));
    app.set_error_handler(|_, _| panic!("handler panicked (intended)"));
    taken(&log);
    app.send(Msg::Named("save"));
    app.send(Msg::Plain);
    assert!(!app.is_stopped());
    assert_eq!(
        to_standard_error(),
        ["the error handler panicked, so the app goes on"]
    );
    assert_eq!(
        taken(&log),
        [
            r#"update Named("save")"#,
            "render",
            "update Plain",
            "render"
        ]
    );
}

#[test]
fn a_report_from_inside_the_handler_goes_to_standard_error() {
    let log = Log::default();
    let told = Rc::new(Cell::new(0));
    let app = start(None, &log, |_| Effect::fail("first"), Rc::new(|| {}));
    let count = told.clone();
    app.set_error_handler(move |_, _| {
        count.set(count.get() + 1);
        // ハンドラの中で起きたものは、ハンドラへ入り直さない。
        crate::error::report(ReportKind::Failure, "from inside");
        Decision::GoOn
    });
    app.send(Msg::Plain);
    assert_eq!(told.get(), 1);
    assert_eq!(to_standard_error(), ["from inside (while handling Plain)"]);
}

#[test]
fn a_message_sent_during_a_panic_waits() {
    struct SendsWhenDropped(Dispatch<Msg>);

    impl Drop for SendsWhenDropped {
        fn drop(&mut self) {
            self.0.send(Msg::FromHandler);
        }
    }

    let (event_loop, tasks) = Loop::new();
    let log = Log::default();
    let app = start(Some(&tasks), &log, |_| Effect::none(), Rc::new(|| {}));
    taken(&log);
    let dispatch = app.dispatch();
    let _ = catch_unwind(AssertUnwindSafe(move || {
        let _sends = SendsWhenDropped(dispatch);
        panic!("unwinding (intended)");
    }));
    assert!(
        taken(&log).is_empty(),
        "巻き戻しの途中では update を呼ばない"
    );
    event_loop.run();
    assert_eq!(taken(&log), ["update FromHandler", "render"]);
}

// メッセージの文字列

/// Counts how much of it is formatted.
struct Huge(Rc<Cell<usize>>);

impl fmt::Debug for Huge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for _ in 0..100_000 {
            self.0.set(self.0.get() + 1);
            f.write_str("é")?;
        }
        Ok(())
    }
}

/// A model whose only message is [`Huge`].
struct OfHuge;

impl Program for OfHuge {
    type Message = Huge;
    type View = ();

    fn update(&mut self, _: Huge) -> Effect<Huge> {
        panic!("update panicked (intended)");
    }

    fn render(&self, _: &()) {}
}

#[test]
fn a_large_message_is_cut_and_stops_being_formatted() {
    let app = App::new(OfHuge, |_| Ok::<_, Infallible>(())).expect("build");
    let seen = Seen::default();
    let recorded = seen.clone();
    app.set_error_handler(move |report, _| {
        recorded.borrow_mut().push(report.clone());
        Decision::GoOn
    });
    let formatted = Rc::new(Cell::new(0));
    app.send(Huge(formatted.clone()));
    let seen = seen.borrow();
    let text = seen[0].message().expect("a message");
    // é は 2 バイトなので、1024 バイト以内で切ると 512 文字。文字の途中では切らない。
    assert_eq!(text.chars().filter(|c| *c == 'é').count(), 512);
    assert!(text.ends_with('…'));
    assert!(formatted.get() < 600, "{} まで作った", formatted.get());
}

#[test]
fn a_message_is_not_formatted_without_a_handler() {
    let app = App::new(OfHuge, |_| Ok::<_, Infallible>(())).expect("build");
    let formatted = Rc::new(Cell::new(0));
    app.send(Huge(formatted.clone()));
    assert_eq!(formatted.get(), 0);
}

#[test]
fn a_short_message_is_kept_whole() {
    assert_eq!(&*crate::error::describe(&Msg::Named("a")), r#"Named("a")"#);
}

// Children::sync

/// A child whose mount can fail.
struct Child;

impl Program for Child {
    type Message = ();
    type View = ();

    fn update(&mut self, _: ()) -> Effect<()> {
        Effect::none()
    }

    fn render(&self, _: &()) {}
}

/// A parent that renders its children with [`Keyed::sync_with`], as `Children::sync` does.
struct Parent {
    keys: Vec<u32>,
    child: Child,
}

#[derive(Debug)]
enum ParentMsg {
    Keys(Vec<u32>),
    FromHandler,
    Child,
}

impl Program for Parent {
    type Message = ParentMsg;
    type View = Keyed<u32, Child>;

    fn update(&mut self, message: ParentMsg) -> Effect<ParentMsg> {
        if let ParentMsg::Keys(keys) = message {
            self.keys = keys;
        }
        Effect::none()
    }

    fn render(&self, view: &Keyed<u32, Child>) {
        let mut mounts = 0;
        view.sync_with(
            self.keys.iter().map(|key| (*key, &self.child)),
            |_| {
                mounts += 1;
                if mounts == 1 {
                    Err("no widgets")
                } else {
                    Ok(())
                }
            },
            |_| {},
        );
    }
}

fn parent() -> App<Parent> {
    App::new(
        Parent {
            keys: Vec::new(),
            child: Child,
        },
        |dispatch: &Dispatch<ParentMsg>| {
            Ok::<_, Infallible>(Keyed::new(dispatch, |_, ()| ParentMsg::Child, |_| {}))
        },
    )
    .expect("build")
}

#[test]
fn a_failed_mount_and_a_repeated_key_reach_the_handler() {
    let app = parent();
    let seen = Seen::default();
    let recorded = seen.clone();
    app.set_error_handler(move |report, _| {
        recorded.borrow_mut().push(report.clone());
        Decision::GoOn
    });
    app.send(ParentMsg::Keys(vec![1, 1]));
    let seen = seen.borrow();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].kind(), ReportKind::RepeatedKey);
    assert_eq!(seen[1].kind(), ReportKind::Mount);
    assert_eq!(seen[1].text(), "could not mount a child: no widgets");
    assert_eq!(seen[1].message(), Some("Keys([1, 1])"));
    assert!(to_standard_error().is_empty());
}

#[test]
fn a_handler_sending_a_message_about_a_failed_mount_does_not_go_round_forever() {
    let app = parent();
    let told = Rc::new(Cell::new(0));
    let count = told.clone();
    app.set_error_handler(move |_, dispatch| {
        count.set(count.get() + 1);
        assert!(count.get() < 10, "止まらない");
        dispatch.send(ParentMsg::FromHandler);
        Decision::GoOn
    });
    app.send(ParentMsg::Keys(vec![1]));
    assert_eq!(told.get(), 1);
}
