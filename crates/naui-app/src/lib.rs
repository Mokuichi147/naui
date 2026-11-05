//! # naui-app
//!
//! A Model–View–Update structure for [naui](https://docs.rs/naui) apps.
//!
//! The app's state lives in one model. Callbacks do not change it; they only
//! **send a typed message**. Messages are handled one at a time, in the order they
//! were sent: [`Program::update`] changes the model and may ask for an [`Effect`], the
//! borrow of the model ends, [`Program::render`] sets the widgets to match it, and only
//! then does the effect start.
//!
//! ```no_run
//! use naui::{Orientation, Settings, Ui};
//! use naui_app::{Dispatch, Effect, Program};
//!
//! #[derive(Default)]
//! struct Counter {
//!     count: i32,
//! }
//!
//! enum Message {
//!     Increment,
//!     Reset,
//! }
//!
//! struct View {
//!     label: naui::Label,
//!     reset: naui::Button,
//! }
//!
//! impl Program for Counter {
//!     type Message = Message;
//!     type View = View;
//!
//!     fn update(&mut self, message: Message) -> Effect<Message> {
//!         match message {
//!             Message::Increment => self.count += 1,
//!             Message::Reset => self.count = 0,
//!         }
//!         Effect::none()
//!     }
//!
//!     fn render(&self, view: &View) {
//!         view.label.set_text(&self.count.to_string());
//!         view.reset.set_enabled(self.count != 0);
//!     }
//! }
//!
//! fn build(ui: &Ui, dispatch: &Dispatch<Message>) -> naui::Result<View> {
//!     let window = ui.window("counter", 320.0, 200.0)?;
//!     let stack = ui.stack(Orientation::Vertical)?;
//!     let label = ui.label("")?;
//!     let increment = ui.button("Increment")?;
//!     increment.on_click(dispatch.to(|| Message::Increment));
//!     let reset = ui.button("Reset")?;
//!     reset.on_click(dispatch.to(|| Message::Reset));
//!     stack.append(&label);
//!     stack.append(&increment);
//!     stack.append(&reset);
//!     window.set_child(&stack);
//!     window.show();
//!     Ok(View { label, reset })
//! }
//!
//! fn main() -> naui::Result<()> {
//!     naui_app::run(Settings::new("counter"), Counter::default(), build)
//! }
//! ```
//!
//! ## What this gives
//!
//! - **Checks at compile time.** A message `update` forgets to handle is a compile
//!   error, since `match` has to be exhaustive.
//! - **No double borrows at run time.** A message sent while another is being
//!   handled (from `render`, from a nested run loop such as macOS's modal
//!   `Dialog::open`, or from a value arriving from another thread) waits in the
//!   queue instead of re-entering `update`, so the model needs no
//!   `Rc<RefCell<_>>`.
//! - **Testable logic.** `update` is plain Rust; it can be called without a window,
//!   children included, and the effects it returns are values a test can look at.
//!   [`App::new`] runs the queue without a window too, with any type as the view.
//!
//! ## Building once, updating with setters
//!
//! The widgets are built once (the `build` function above), and `render` calls the
//! setters it needs every time. There is no virtual widget tree and no diffing.
//! naui's setters do nothing when given the value already shown, so setting the
//! text of a field being typed into does not move the caret or break the IME
//! composition.
//!
//! `render` runs once after the queue empties, not once per message: several
//! messages sent together are all handed to `update` first.
//!
//! ## Splitting into components
//!
//! A screen, a panel or a row can be a [`Component`]: its own model, messages,
//! `update` and widgets. Its model lives in the parent's model, and its messages
//! are wrapped into the parent's (`Message::Editor(editor::Message)`), so they go
//! through the same queue and the parent's `update` hands them on to the child's.
//! The parent's view keeps its widgets: a [`Mounted`] for one that is always
//! there, [`Children`] for ones that come and go with a collection in the model.
//! Taking a child down ([`Mounted::unmount`], or [`Children::sync`] once its key is
//! gone) disconnects its dispatch, calls [`Component::unmount`], has the parent
//! remove its root widget, and drops the widgets, in that order. Components nested
//! in its view are taken down with it, when those widgets are dropped. See
//! [`examples/checklist`](https://github.com/mokuichi147/naui/tree/main/examples/checklist).
//!
//! ## Effects
//!
//! `update` does not start work. It returns an [`Effect`] that asks for it: a task
//! ([`Effect::spawn`]), work on other threads ([`Effect::thread`],
//! [`Effect::channel`]), a timer ([`Effect::after`], [`Effect::every`]) or a failure to
//! report ([`Effect::fail`]). Results come back as messages, through the same queue.
//! Work to do when the app or a component starts is asked for in the same way: send a
//! message from `build` or [`Component::mount`], and return the effect from its
//! `update`.
//!
//! A parent hands a child's effect on with [`Effect::map`], as it wraps the child's
//! messages. An effect belongs to the model that asked for it: a child's model is kept
//! in a [`Scoped`] wrapper, and dropping the wrapper cancels what the child started.
//! Effects under the same [`key`](Effect::key) in one model replace each other (a
//! search overtaken by further typing). See
//! [`examples/search`](https://github.com/mokuichi147/naui/tree/main/examples/search).
//!
//! ## When something goes wrong
//!
//! One handler, set with [`run_with_handler`] or [`App::set_error_handler`], receives a
//! [`Report`] of each of these on the UI thread:
//!
//! - panics in `update`, `render` and effects starting, and in any other naui callback,
//! - failures: [`Effect::fail`], and an effect's thread that panicked or did not start,
//! - children [`Children::sync`] could not mount, and keys it was given twice.
//!
//! Without a handler the failures, the mounts and the keys go to standard error, and a
//! panic goes where the previous panic hook sends it.
//!
//! A report from inside the queue says which message was being handled
//! ([`Report::message`]), which is why the handler needs the message type to be
//! [`Debug`]. The text is recorded before `update` (which takes the message away), cut
//! to 1024 bytes, and only while a handler is set, so large messages cost little.
//!
//! ### Panics
//!
//! On the desktop platforms with `panic = "unwind"`, a panic is caught per message, per
//! `render` and per effect started. naui-app cannot tell whether the model is still
//! whole, so the handler decides: [`Decision::GoOn`] carries on with the model as the
//! panic left it (then `render` runs, as after any message), and [`Decision::Stop`]
//! stops the app. Without a handler the queue goes on.
//!
//! On the Web (wasm aborts on a panic) and with `panic = "abort"` nothing can be caught.
//! The handler is told from inside the panic hook and the app stops, whatever it answers.
//!
//! A panic outside the queue (in a callback that is not a [`Dispatch`], say) reaches the
//! handler through the panic hook as well, without a message. With several apps on a
//! thread, each hears about its own queue and effects, and such a panic goes to the
//! handler set last. naui-app installs that hook when a handler is first set, in front of
//! the one already there, which still runs. An app that sets a panic hook of its own
//! afterwards replaces both.
//!
//! ### Writing a handler
//!
//! It gets a [`Dispatch`], and showing a failure is best done with a message: `update`
//! changes the model, and `render` shows it. Messages it sends wait in the queue. A
//! report from outside the queue comes while the panic is still going, so there the
//! handler must not panic (that aborts the process) and is better off not touching
//! widgets, whose state the panic may have left half changed.
//!
//! A panic that a message of the handler's brings back (a `render` that panics every
//! time, with a handler that always sends something) goes round forever. Return
//! [`Decision::Stop`] for panics, or send only once.
//!
//! ```no_run
//! # use naui::{Settings, Ui};
//! # use naui_app::{Decision, Dispatch, Effect, Program, Report, ReportKind};
//! # #[derive(Default)] struct Model { problem: String }
//! # #[derive(Debug)] enum Message { Problem(String) }
//! # impl Program for Model {
//! #     type Message = Message;
//! #     type View = ();
//! #     fn update(&mut self, _: Message) -> Effect<Message> { Effect::none() }
//! #     fn render(&self, _: &()) {}
//! # }
//! # fn build(_: &Ui, _: &Dispatch<Message>) -> naui::Result<()> { Ok(()) }
//! fn on_error(report: &Report, dispatch: &Dispatch<Message>) -> Decision {
//!     eprintln!("{report}");
//!     if report.kind() == ReportKind::Panic {
//!         // The model may be half changed. Do not go on with it.
//!         return Decision::Stop;
//!     }
//!     dispatch.send(Message::Problem(report.text().to_owned()));
//!     Decision::GoOn
//! }
//!
//! fn main() -> naui::Result<()> {
//!     naui_app::run_with_handler(Settings::new("app"), Model::default(), build, on_error)
//! }
//! ```

#![forbid(unsafe_code)]

use std::cell::{Cell, OnceCell, Ref, RefCell};
use std::collections::VecDeque;
use std::fmt::Debug;
use std::rc::{Rc, Weak};
use std::time::Duration;

use naui::{Sender, Settings, Tasks, Ui};

/// The model of an app (or of a screen), with how messages change it and how it is
/// shown.
///
/// The type implementing it is the model itself.
pub trait Program: 'static {
    /// What callbacks send: a click, a changed input, a finished load.
    type Message: 'static;

    /// The widgets `render` updates, built once. Usually a struct of widget handles.
    type View: 'static;

    /// Changes the model for one message.
    ///
    /// It does not touch widgets; that is `render`'s job, after the change. Nor does it
    /// start work: it returns an [`Effect`] asking for it ([`Effect::none`] if there is
    /// nothing to do), which starts after the next `render`. Messages sent from here
    /// (through a [`Dispatch`] the model holds) wait in the queue and are handled after
    /// this one.
    fn update(&mut self, message: Self::Message) -> Effect<Self::Message>;

    /// Sets the widgets to match the model.
    ///
    /// Called once after the view is built, and again whenever the queue has been
    /// emptied by `update`, before the effects `update` returned start. Call every
    /// setter that depends on the model every time:
    /// naui's setters do nothing when the value is already shown.
    fn render(&self, view: &Self::View);
}

/// What a [`Dispatch`] sends into.
trait Mailbox<M> {
    fn post(&self, message: M);
    fn is_connected(&self) -> bool;
    /// Whether the app is still there, whether or not a component in between is
    /// still mounted.
    fn app_is_there(&self) -> bool;
}

/// An app's own mailbox. It refers to the app weakly.
struct ToApp<P: Program>(Weak<Inner<P>>);

impl<P: Program> Mailbox<P::Message> for ToApp<P> {
    fn post(&self, message: P::Message) {
        if let Some(inner) = self.0.upgrade() {
            inner.post(message);
        }
    }

    fn is_connected(&self) -> bool {
        self.app_is_there()
    }

    fn app_is_there(&self) -> bool {
        self.0.strong_count() > 0
    }
}

/// Wraps a child's messages into its parent's. With a link, it sends only while the
/// link is alive (while the child is mounted).
struct Wrapped<M: 'static, F> {
    parent: Dispatch<M>,
    wrap: F,
    link: Option<Weak<()>>,
}

impl<M: 'static, C, F: Fn(C) -> M> Mailbox<C> for Wrapped<M, F> {
    fn post(&self, message: C) {
        if self.is_connected() {
            self.parent.send((self.wrap)(message));
        }
    }

    fn is_connected(&self) -> bool {
        self.link
            .as_ref()
            .is_none_or(|link| link.strong_count() > 0)
            && self.parent.is_connected()
    }

    fn app_is_there(&self) -> bool {
        self.parent.mailbox.app_is_there()
    }
}

/// Sends messages to an [`App`]. Clone it into callbacks.
///
/// It refers to the app weakly, so the widgets holding it (in their callbacks) do
/// not keep the app alive; once the app is gone, sending does nothing.
///
/// A component's dispatch, made with [`wrap`](Self::wrap) or given to its
/// [`mount`](Component::mount), sends into the same queue: what it sends is wrapped
/// into the parent's message and handed to the parent's `update`.
///
/// It is not `Send`. To send from another thread, make a [`Sender`] with
/// [`sender`](Self::sender).
pub struct Dispatch<M: 'static> {
    mailbox: Rc<dyn Mailbox<M>>,
}

impl<M: 'static> Clone for Dispatch<M> {
    fn clone(&self) -> Self {
        Self {
            mailbox: self.mailbox.clone(),
        }
    }
}

impl<M: 'static> std::fmt::Debug for Dispatch<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dispatch")
            .field("connected", &self.is_connected())
            .finish()
    }
}

impl<M: 'static> Dispatch<M> {
    /// Puts a message in the queue, and handles the queue unless it is already being
    /// handled (then the message waits its turn).
    ///
    /// Does nothing once the app is gone, or once the component this dispatch was made
    /// for is unmounted.
    pub fn send(&self, message: M) {
        self.mailbox.post(message);
    }

    /// Whether the app is still there to receive messages (and, for a component's
    /// dispatch, whether the component is still mounted).
    pub fn is_connected(&self) -> bool {
        self.mailbox.is_connected()
    }

    /// A dispatch for a child, whose messages are wrapped with `wrap` into this
    /// one's. For example `dispatch.wrap(Message::Editor)` when the parent's message
    /// has a variant `Editor(editor::Message)`.
    ///
    /// The parent's `update` receives the wrapped message and hands it on to the
    /// child's `update` (or acts on it itself first). Everything still goes through
    /// one queue, so the guarantee against re-entry covers the children too.
    ///
    /// The dispatch made here stays connected as long as the app is there. One that
    /// stops with its child is made by [`Mounted`] and [`Children`].
    pub fn wrap<C: 'static>(&self, wrap: impl Fn(C) -> M + 'static) -> Dispatch<C> {
        self.wrapped(wrap, None)
    }

    /// Like [`wrap`](Self::wrap), sending only while `link` is alive.
    fn wrapped<C: 'static>(
        &self,
        wrap: impl Fn(C) -> M + 'static,
        link: Option<Weak<()>>,
    ) -> Dispatch<C> {
        Dispatch {
            mailbox: Rc::new(Wrapped {
                parent: self.clone(),
                wrap,
                link,
            }),
        }
    }

    /// A closure that sends the message `make` returns. For callbacks without a
    /// value, such as `on_click`.
    ///
    /// ```no_run
    /// # enum Message { Save }
    /// # fn wire(button: &naui::Button, dispatch: &naui_app::Dispatch<Message>) {
    /// button.on_click(dispatch.to(|| Message::Save));
    /// # }
    /// ```
    pub fn to(&self, mut make: impl FnMut() -> M + 'static) -> impl FnMut() + 'static {
        let dispatch = self.clone();
        move || dispatch.send(make())
    }

    /// A closure that turns the value a callback receives into a message and sends
    /// it. For callbacks such as `on_change`.
    ///
    /// ```no_run
    /// # enum Message { NameChanged(String) }
    /// # fn wire(field: &naui::TextInput, dispatch: &naui_app::Dispatch<Message>) {
    /// field.on_change(dispatch.with(|text: &str| Message::NameChanged(text.to_owned())));
    /// # }
    /// ```
    pub fn with<T: ?Sized>(
        &self,
        mut make: impl FnMut(&T) -> M + 'static,
    ) -> impl FnMut(&T) + 'static {
        let dispatch = self.clone();
        move |value| dispatch.send(make(value))
    }

    /// Like [`with`](Self::with), for callbacks that receive a value by itself
    /// (`on_toggle(bool)`, `on_select(usize)` and so on).
    pub fn map<T>(&self, mut make: impl FnMut(T) -> M + 'static) -> impl FnMut(T) + 'static {
        let dispatch = self.clone();
        move |value| dispatch.send(make(value))
    }

    /// A [`Sender`] for other threads. What it sends arrives on the UI thread (through
    /// [`Tasks::channel`]) and joins the queue from there, after the messages already
    /// in it.
    pub fn sender(&self, tasks: &Tasks) -> Sender<M>
    where
        M: Send,
    {
        let dispatch = self.clone();
        tasks.channel(move |message| dispatch.send(message))
    }
}

/// An effect waiting to start, with the text of the message it was returned for.
type Waiting<M> = (Option<Rc<str>>, Effect<M>);

/// Makes the text of a message.
type Describe<M> = fn(&M) -> Rc<str>;

struct Inner<P: Program> {
    model: RefCell<P>,
    view: OnceCell<P::View>,
    queue: RefCell<VecDeque<P::Message>>,
    /// What `update` returned, waiting for the next `render` to be over.
    effects: RefCell<VecDeque<Waiting<P::Message>>>,
    /// What the effects run on. `None` when the app was made without it.
    tasks: Option<Tasks>,
    /// Owns the effects that belong to the app's own model. A child's belong to its
    /// [`Scoped`] wrapper, and those go first, with the model.
    owner: Owner,
    /// Holds everything this app has started, children's too, for when it stops.
    started: Owner,
    /// Reports a failure to this app's handler, from outside its queue.
    report: Rc<dyn Fn(&str)>,
    /// A dispatch to this app, for what effects send.
    dispatch: Dispatch<P::Message>,
    /// This app, for what runs later.
    me: Weak<Inner<P>>,
    /// The handler, when this app set one. Only a weak reference is kept elsewhere, so it
    /// goes with the app.
    handler: RefCell<Option<Rc<error::Slot>>>,
    /// Makes the text of a message. Set together with the handler, since that is what
    /// asks for `Debug`.
    describe: Cell<Option<Describe<P::Message>>>,
    /// Quits the UI when the app stops, if the app knows the UI.
    quit: RefCell<Option<Box<dyn FnOnce()>>>,
    /// Whether the handler stopped the app. No message is handled from then on.
    stopped: Cell<bool>,
    /// Whether the queue is being handled. A message posted meanwhile waits.
    running: Cell<bool>,
    /// Whether the model may have changed since the last render.
    dirty: Cell<bool>,
}

/// Clears a flag when dropped, even during a panic that something outside the queue
/// catches.
struct Running<'a>(&'a Cell<bool>);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl<P: Program> Inner<P> {
    fn post(&self, message: P::Message) {
        if self.stopped.get() {
            return;
        }
        self.queue.borrow_mut().push_back(message);
        self.run();
    }

    /// Hands every queued message to `update`, renders, and then starts the effects
    /// `update` returned, until nothing is left to do. Does nothing if already running
    /// (the caller up the stack carries on), before the view is built, or once stopped.
    ///
    /// A panic in any of these is caught and reported, and the handler decides what
    /// happens next. Nothing unwinds out of here, so nothing unwinds into the native
    /// code that called the callback.
    fn run(&self) {
        let Some(view) = self.view.get() else {
            return;
        };
        if self.stopped.get() || self.running.get() {
            return;
        }
        // panic の最中に update を走らせると、次の panic でプロセスが落ちる。
        if error::must_wait() {
            self.run_later();
            return;
        }
        self.running.set(true);
        let _running = Running(&self.running);
        let _queue = self.queue_scope();
        loop {
            if self.stopped.get() {
                self.abandon();
                break;
            }
            // 取り出す間だけキューを借りる。update の中から送られたものは、
            // 借りていないキューへ積まれて、この同じループで処理される。
            if let Some(message) = self.pop() {
                error::catch(|| {
                    // update がメッセージを持っていくので、文字列は先に作っておく。
                    let text = self.describe.get().map(|describe| describe(&message));
                    error::set_current_message(text.clone());
                    let effect = self.model.borrow_mut().update(message);
                    self.effects.borrow_mut().push_back((text, effect));
                });
                self.dirty.set(true);
                continue;
            }
            if self.dirty.replace(false) {
                // render が値を変えて通知を起こし、メッセージが積まれることがある。
                // それはもう一周して update へ渡す。効果を始めるのは落ち着いてから。
                error::catch(|| self.model.borrow().render(view));
                continue;
            }
            // モデルを借りていないところで、ひとつずつ始める。
            let next = self.effects.borrow_mut().pop_front();
            let Some((text, effect)) = next else {
                break;
            };
            error::set_current_message(text);
            error::catch(|| {
                let starting = Starting {
                    tasks: self.tasks.as_ref(),
                    dispatch: &self.dispatch,
                    stopped: &self.stopped,
                    started: &self.started,
                    report: &self.report,
                };
                effect.start(&starting, &self.owner);
            });
        }
    }

    /// From here on, reports go to this app's handler, or without one to standard error,
    /// and not to the handler of another app on the thread.
    fn queue_scope(&self) -> error::QueueScope {
        let slot = self
            .handler
            .borrow()
            .as_ref()
            .map_or_else(Weak::new, Rc::downgrade);
        error::QueueScope::enter(slot)
    }

    /// Runs the queue from the event loop. Without [`Tasks`], the next message does.
    fn run_later(&self) {
        if let Some(tasks) = &self.tasks {
            let me = self.me.clone();
            tasks.after(Duration::ZERO, move || {
                if let Some(inner) = me.upgrade() {
                    inner.run();
                }
            });
        }
    }

    /// Stops handling messages. This can come from inside the panic hook, so the rest
    /// waits for the event loop, where widgets and the app's code can run.
    fn stop(&self) {
        if self.stopped.replace(true) {
            return;
        }
        if let Some(tasks) = &self.tasks {
            let me = self.me.clone();
            tasks.after(Duration::ZERO, move || {
                if let Some(inner) = me.upgrade() {
                    inner.wind_down();
                }
            });
        }
    }

    fn wind_down(&self) {
        let _queue = self.queue_scope();
        self.abandon();
        self.started.cancel_all();
        let quit = self.quit.borrow_mut().take();
        if let Some(quit) = quit {
            quit();
        }
    }

    /// Drops what is waiting. A failure among the effects has happened all the same, so
    /// it is still reported.
    fn abandon(&self) {
        let messages = std::mem::take(&mut *self.queue.borrow_mut());
        drop(messages);
        let effects = std::mem::take(&mut *self.effects.borrow_mut());
        let _message = error::MessageScope::enter();
        for (text, effect) in effects {
            error::set_current_message(text);
            for failure in effect.failures() {
                report(ReportKind::Failure, failure);
            }
        }
    }

    /// Sets the handler, and starts recording the text of each message.
    fn set_error_handler(
        &self,
        mut handler: impl FnMut(&Report, &Dispatch<P::Message>) -> Decision + 'static,
    ) where
        P::Message: Debug,
    {
        let dispatch = self.dispatch.clone();
        let me = self.me.clone();
        let slot = error::Slot::new(
            Box::new(move |report| handler(report, &dispatch)),
            Box::new(move || {
                if let Some(inner) = me.upgrade() {
                    inner.stop();
                }
            }),
        );
        error::set_slot(&slot);
        // 動いている最中のキューは、もう古いハンドラを指している。
        if self.running.get() {
            error::set_queue_slot(&slot);
        }
        self.describe.set(Some(error::describe::<P::Message>));
        let previous = self.handler.borrow_mut().replace(slot);
        drop(previous);
    }

    fn pop(&self) -> Option<P::Message> {
        self.queue.borrow_mut().pop_front()
    }
}

/// A running [`Program`]: the model, its view, and the queue of messages.
///
/// Dropping it stops it: [`Dispatch`]es made from it then send nothing. Inside
/// [`run`] the app is kept until the UI finishes; with [`App::new`], keep it
/// yourself.
pub struct App<P: Program> {
    inner: Rc<Inner<P>>,
}

impl<P: Program> App<P> {
    /// Starts `model`: builds the view with `build`, which receives the
    /// [`Dispatch`] to wire into callbacks, then renders once.
    ///
    /// Messages sent while `build` runs wait until the first render, and are then
    /// handled in order.
    ///
    /// Nothing here needs a window, so tests can use any type as the view. Without
    /// [`Tasks`] effects cannot run, though: a failure is still reported, and work is
    /// reported as a failure instead of starting. An app that runs effects inside
    /// [`naui::run`] is made with [`with_tasks`](Self::with_tasks).
    pub fn new<E>(
        model: P,
        build: impl FnOnce(&Dispatch<P::Message>) -> Result<P::View, E>,
    ) -> Result<Self, E> {
        Self::start(None, None, |_| {}, model, build)
    }

    /// Like [`new`](Self::new), running effects on `tasks` ([`Ui::tasks`]).
    ///
    /// When the handler stops this app its queue stops, but the UI stays: it is the
    /// caller's. Only [`run`] and [`run_with_handler`] quit it as well.
    pub fn with_tasks<E>(
        tasks: &Tasks,
        model: P,
        build: impl FnOnce(&Dispatch<P::Message>) -> Result<P::View, E>,
    ) -> Result<Self, E> {
        Self::start(Some(tasks.clone()), None, |_| {}, model, build)
    }

    fn start<E>(
        tasks: Option<Tasks>,
        quit: Option<Box<dyn FnOnce()>>,
        set_up: impl FnOnce(&Inner<P>),
        model: P,
        build: impl FnOnce(&Dispatch<P::Message>) -> Result<P::View, E>,
    ) -> Result<Self, E> {
        let inner = Rc::new_cyclic(|me: &Weak<Inner<P>>| Inner {
            model: RefCell::new(model),
            view: OnceCell::new(),
            queue: RefCell::new(VecDeque::new()),
            effects: RefCell::new(VecDeque::new()),
            tasks,
            owner: Owner::default(),
            started: Owner::default(),
            report: {
                let me = me.clone();
                Rc::new(move |failure: &str| {
                    if let Some(inner) = me.upgrade() {
                        let _queue = inner.queue_scope();
                        report(ReportKind::Failure, failure);
                    }
                })
            },
            dispatch: Dispatch {
                mailbox: Rc::new(ToApp(me.clone())),
            },
            me: me.clone(),
            handler: RefCell::new(None),
            describe: Cell::new(None),
            quit: RefCell::new(quit),
            stopped: Cell::new(false),
            running: Cell::new(false),
            dirty: Cell::new(true),
        });
        // build の panic もハンドラへ届くよう、先に設定する。
        set_up(&inner);
        let app = Self { inner };
        let view = build(&app.dispatch())?;
        if app.inner.view.set(view).is_err() {
            unreachable!("the view is set only here");
        }
        // 最初の render も、処理中の印を立ててから行う (`dirty` は立っている)。
        app.inner.run();
        Ok(app)
    }

    /// Sets the one handler for what goes wrong, replacing the one set before. See
    /// [When something goes wrong](crate#when-something-goes-wrong).
    ///
    /// The handler belongs to this app and goes when the app is dropped. It hears about
    /// this app's queue and effects, and no other app's, even on the same thread. A
    /// panic outside any queue cannot be tied to an app, so it goes to the handler set
    /// last among those still there, and [`Decision::Stop`] stops the app that set it.
    ///
    /// It takes effect from here on. To hear about `build` and the first `render` as
    /// well, use [`run_with_handler`].
    pub fn set_error_handler(
        &self,
        handler: impl FnMut(&Report, &Dispatch<P::Message>) -> Decision + 'static,
    ) where
        P::Message: Debug,
    {
        self.inner.set_error_handler(handler);
    }

    /// Whether the handler stopped this app ([`Decision::Stop`]). Messages sent from then
    /// on are dropped.
    pub fn is_stopped(&self) -> bool {
        self.inner.stopped.get()
    }

    /// A [`Dispatch`] for sending messages to this app.
    pub fn dispatch(&self) -> Dispatch<P::Message> {
        self.inner.dispatch.clone()
    }

    /// Sends a message, as [`Dispatch::send`] does.
    pub fn send(&self, message: P::Message) {
        self.inner.post(message);
    }

    /// The model, for reading. Do not hold it across a [`send`](Self::send):
    /// `update` needs to borrow the model mutably.
    pub fn model(&self) -> Ref<'_, P> {
        self.inner.model.borrow()
    }

    /// The view built at the start.
    pub fn view(&self) -> &P::View {
        self.inner.view.get().expect("the view is built in `new`")
    }

    /// The number of messages waiting in the queue (other than while it is being
    /// handled, always 0).
    pub fn pending(&self) -> usize {
        self.inner.queue.borrow().len()
    }

    /// How many effects of the app's own model have started and not finished. Those of
    /// a child in a [`Scoped`] are counted by its [`Scoped::running`].
    pub fn running(&self) -> usize {
        self.inner.owner.running()
    }
}

thread_local! {
    /// The apps [`run`] keeps until the UI finishes.
    static RUNNING: RefCell<Vec<Box<dyn std::any::Any>>> = const { RefCell::new(Vec::new()) };
}

/// Starts naui and runs `model` in it.
///
/// `build` creates the window and the widgets once, wiring their callbacks to the
/// [`Dispatch`] it receives, and returns the view `render` updates. The app is kept
/// until the UI finishes. Like [`naui::run`], this does not return until every
/// window has closed (on the Web it returns right away and the page keeps running).
pub fn run<P, F>(settings: Settings, model: P, build: F) -> naui::Result<()>
where
    P: Program,
    F: FnOnce(&Ui, &Dispatch<P::Message>) -> naui::Result<P::View> + 'static,
{
    run_app(settings, model, build, |_| {})
}

/// Like [`run`], with a handler for what goes wrong from the start, `build` included.
/// See [When something goes wrong](crate#when-something-goes-wrong).
///
/// When it answers [`Decision::Stop`], the queue stops and the UI quits. On the Web a page
/// cannot quit, so it stays as it is with nothing handling its messages.
pub fn run_with_handler<P, F, H>(
    settings: Settings,
    model: P,
    build: F,
    handler: H,
) -> naui::Result<()>
where
    P: Program,
    P::Message: Debug,
    F: FnOnce(&Ui, &Dispatch<P::Message>) -> naui::Result<P::View> + 'static,
    H: FnMut(&Report, &Dispatch<P::Message>) -> Decision + 'static,
{
    run_app(settings, model, build, move |inner: &Inner<P>| {
        inner.set_error_handler(handler);
    })
}

fn run_app<P, F>(
    settings: Settings,
    model: P,
    build: F,
    set_up: impl FnOnce(&Inner<P>) + 'static,
) -> naui::Result<()>
where
    P: Program,
    F: FnOnce(&Ui, &Dispatch<P::Message>) -> naui::Result<P::View> + 'static,
{
    let result = naui::run(settings, move |ui| {
        let quitting = ui.clone();
        let app = App::start(
            Some(ui.tasks()),
            Some(Box::new(move || quitting.quit())),
            set_up,
            model,
            |dispatch| build(ui, dispatch),
        )?;
        RUNNING.with(|apps| apps.borrow_mut().push(Box::new(app)));
        Ok(())
    });
    // Web では run がすぐ戻り、ページはそのまま動き続けるので、アプリを残す。
    if !cfg!(target_arch = "wasm32") {
        let apps = RUNNING.with(|apps| std::mem::take(&mut *apps.borrow_mut()));
        drop(apps);
    }
    result
}

mod component;
mod effect;
mod error;

pub use component::{Change, Children, Component, Mounted};
pub use effect::{Effect, Scoped};
pub use error::{Decision, Report, ReportKind};

use effect::{Owner, Starting};
use error::report;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod component_tests;

#[cfg(test)]
mod effect_tests;

#[cfg(test)]
mod error_tests;
