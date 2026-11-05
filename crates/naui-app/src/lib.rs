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
//! ## Not yet here
//!
//! One handler for panics and errors is the next step on naui's roadmap. Until then,
//! a failure goes to standard error.

#![forbid(unsafe_code)]

use std::cell::{Cell, OnceCell, Ref, RefCell};
use std::collections::VecDeque;
use std::rc::{Rc, Weak};

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

struct Inner<P: Program> {
    /// Owns the effects that belong to the app's own model. A child's belong to its
    /// [`Scoped`] wrapper. It goes before the model, so that a `Drop` of the model can
    /// wait for work on another thread that watches [`Sender::is_open`].
    owner: Owner,
    model: RefCell<P>,
    view: OnceCell<P::View>,
    queue: RefCell<VecDeque<P::Message>>,
    /// What `update` returned, waiting for the next `render` to be over.
    effects: RefCell<VecDeque<Effect<P::Message>>>,
    /// What the effects run on. `None` when the app was made without it.
    tasks: Option<Tasks>,
    /// A dispatch to this app, for what effects send.
    dispatch: Dispatch<P::Message>,
    /// Whether the queue is being handled. A message posted meanwhile waits.
    running: Cell<bool>,
    /// Whether the model may have changed since the last render.
    dirty: Cell<bool>,
}

/// Clears the running flag even when `update` or `render` panics, so the messages
/// sent after that are handled.
struct Running<'a>(&'a Cell<bool>);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl<P: Program> Inner<P> {
    fn post(&self, message: P::Message) {
        self.queue.borrow_mut().push_back(message);
        self.run();
    }

    /// Hands every queued message to `update`, renders, and then starts the effects
    /// `update` returned, until nothing is left to do. Does nothing if already running
    /// (the caller up the stack carries on) or before the view is built.
    fn run(&self) {
        let Some(view) = self.view.get() else {
            return;
        };
        if self.running.replace(true) {
            return;
        }
        let _running = Running(&self.running);
        loop {
            // 取り出す間だけキューを借りる。update の中から送られたものは、
            // 借りていないキューへ積まれて、この同じループで処理される。
            while let Some(message) = self.pop() {
                let effect = self.model.borrow_mut().update(message);
                self.effects.borrow_mut().push_back(effect);
                self.dirty.set(true);
            }
            if self.dirty.replace(false) {
                // render が値を変えて通知を起こし、メッセージが積まれることがある。
                // それはもう一周して update へ渡す。効果を始めるのは落ち着いてから。
                self.model.borrow().render(view);
                continue;
            }
            // モデルを借りていないところで、ひとつずつ始める。panic しても残りは
            // effects に残り、次に動くときに始まる。
            let effect = self.effects.borrow_mut().pop_front();
            let Some(effect) = effect else {
                break;
            };
            effect.start(self.tasks.as_ref(), &self.dispatch, &self.owner);
        }
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
        Self::start(None, model, build)
    }

    /// Like [`new`](Self::new), running effects on `tasks` ([`Ui::tasks`]).
    pub fn with_tasks<E>(
        tasks: &Tasks,
        model: P,
        build: impl FnOnce(&Dispatch<P::Message>) -> Result<P::View, E>,
    ) -> Result<Self, E> {
        Self::start(Some(tasks.clone()), model, build)
    }

    fn start<E>(
        tasks: Option<Tasks>,
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
            dispatch: Dispatch {
                mailbox: Rc::new(ToApp(me.clone())),
            },
            running: Cell::new(false),
            dirty: Cell::new(true),
        });
        let app = Self { inner };
        let view = build(&app.dispatch())?;
        if app.inner.view.set(view).is_err() {
            unreachable!("the view is set only here");
        }
        // 最初の render も、処理中の印を立ててから行う (`dirty` は立っている)。
        app.inner.run();
        Ok(app)
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
    let result = naui::run(settings, move |ui| {
        let app = App::with_tasks(&ui.tasks(), model, |dispatch| build(ui, dispatch))?;
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

pub use component::{Change, Children, Component, Mounted};
pub use effect::{Effect, Scoped};

use effect::Owner;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod component_tests;

#[cfg(test)]
mod effect_tests;
