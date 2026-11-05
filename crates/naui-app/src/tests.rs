//! The queue's order and its guarantee against re-entry, without a window.

use std::cell::RefCell;
use std::convert::Infallible;
use std::rc::Rc;

use super::{App, Dispatch, Effect, Program};

/// What happened, in order: `update` and `render` calls.
type Log = Rc<RefCell<Vec<String>>>;

/// A model that records every call, and can send more messages from `update` and
/// from `render` through a dispatch it holds.
struct Recorder {
    total: i32,
    log: Log,
    dispatch: Option<Dispatch<Msg>>,
    /// Messages the next `render` sends, once (as a native notification raised by
    /// a setter would).
    from_render: RefCell<Vec<Msg>>,
}

#[derive(Debug, Clone, PartialEq)]
enum Msg {
    Add(i32),
    /// Sends `Add(n)` from inside `update`.
    Chain(i32),
    Panic,
}

/// The view: what `render` writes, as a widget would show it.
#[derive(Default)]
struct Shown(RefCell<Vec<i32>>);

impl Program for Recorder {
    type Message = Msg;
    type View = Shown;

    fn update(&mut self, message: Msg) -> Effect<Msg> {
        self.log.borrow_mut().push(format!("update {message:?}"));
        match message {
            Msg::Add(n) => self.total += n,
            Msg::Chain(n) => {
                // update の中から送っても、update へ入り直さない。
                self.dispatch.as_ref().expect("dispatch").send(Msg::Add(n));
                self.log.borrow_mut().push("sent from update".into());
            }
            Msg::Panic => panic!("update panicked (intended)"),
        }
        Effect::none()
    }

    fn render(&self, view: &Shown) {
        self.log.borrow_mut().push(format!("render {}", self.total));
        view.0.borrow_mut().push(self.total);
        if let Some(dispatch) = &self.dispatch {
            for message in self.from_render.take() {
                dispatch.send(message);
            }
        }
    }
}

fn recorder(log: &Log) -> Recorder {
    Recorder {
        total: 0,
        log: log.clone(),
        dispatch: None,
        from_render: RefCell::new(Vec::new()),
    }
}

fn start(model: Recorder) -> App<Recorder> {
    let app = App::new(model, |_| Ok::<_, Infallible>(Shown::default())).expect("build");
    // モデルが自分の送り口を持つ (効果を自前で起こすアプリの形)。
    let dispatch = app.dispatch();
    app.inner.model.borrow_mut().dispatch = Some(dispatch);
    app
}

#[test]
fn renders_once_after_building() {
    let log = Log::default();
    let app = start(recorder(&log));
    assert_eq!(*log.borrow(), ["render 0"]);
    assert_eq!(*app.view().0.borrow(), [0]);
}

#[test]
fn messages_update_then_render() {
    let log = Log::default();
    let app = start(recorder(&log));
    app.send(Msg::Add(2));
    app.dispatch().send(Msg::Add(3));
    assert_eq!(
        *log.borrow(),
        [
            "render 0",
            "update Add(2)",
            "render 2",
            "update Add(3)",
            "render 5"
        ]
    );
    assert_eq!(app.model().total, 5);
    assert_eq!(app.pending(), 0);
}

#[test]
fn a_message_sent_from_update_waits_its_turn() {
    let log = Log::default();
    let app = start(recorder(&log));
    log.borrow_mut().clear();
    app.send(Msg::Chain(4));
    // Chain の update が終わってから Add の update が走り、render は最後に 1 回。
    assert_eq!(
        *log.borrow(),
        [
            "update Chain(4)",
            "sent from update",
            "update Add(4)",
            "render 4"
        ]
    );
}

#[test]
fn a_message_sent_from_render_is_handled_after_it() {
    let log = Log::default();
    let app = start(recorder(&log));
    *app.model().from_render.borrow_mut() = vec![Msg::Add(1)];
    log.borrow_mut().clear();
    app.send(Msg::Add(10));
    // render の中で送られたものは、render が終わってから update へ渡る。
    assert_eq!(
        *log.borrow(),
        ["update Add(10)", "render 10", "update Add(1)", "render 11"]
    );
}

#[test]
fn messages_sent_while_building_wait_for_the_first_render() {
    let log = Log::default();
    let app = App::new(recorder(&log), |dispatch: &Dispatch<Msg>| {
        dispatch.send(Msg::Add(1));
        dispatch.send(Msg::Add(2));
        assert!(dispatch.is_connected());
        Ok::<_, Infallible>(Shown::default())
    })
    .expect("build");
    // 組み立ての間に送ったものは、最初の render より先に update へ渡る。
    assert_eq!(
        *log.borrow(),
        ["update Add(1)", "update Add(2)", "render 3"]
    );
    assert_eq!(app.model().total, 3);
}

#[test]
fn a_failing_build_returns_its_error() {
    let log = Log::default();
    let result = App::new(recorder(&log), |_| Err::<Shown, _>("no window"));
    assert_eq!(result.err(), Some("no window"));
    assert!(log.borrow().is_empty(), "render は呼ばれない");
}

#[test]
fn the_queue_keeps_working_after_a_panic() {
    let log = Log::default();
    let app = start(recorder(&log));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app.send(Msg::Panic);
    }));
    assert!(outcome.is_err());
    app.send(Msg::Add(7));
    assert_eq!(app.model().total, 7, "panic のあとも送ったものが届く");
}

#[test]
fn a_dispatch_outliving_its_app_sends_nothing() {
    let log = Log::default();
    let app = start(recorder(&log));
    let dispatch = app.dispatch();
    drop(app);
    assert!(!dispatch.is_connected());
    dispatch.send(Msg::Add(1));
}

#[test]
fn closures_from_dispatch_send_messages() {
    let log = Log::default();
    let app = start(recorder(&log));
    let dispatch = app.dispatch();
    let mut click = dispatch.to(|| Msg::Add(1));
    let mut change = dispatch.with(|text: &str| Msg::Add(text.len() as i32));
    let mut select = dispatch.map(Msg::Add);
    click();
    change("abc");
    select(10);
    assert_eq!(app.model().total, 14);
}
