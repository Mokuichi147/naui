//! Effects: asynchronous work `update` asks for, and the models that own it.

use std::any::Any;
use std::borrow::Borrow;
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::rc::{Rc, Weak};
use std::time::Duration;

use naui::{ChannelCloser, Sender, Task, Tasks, Timer};

use crate::component::attempt;
use crate::error::{panic_text, report};
use crate::{Dispatch, Program, ReportKind};

/// What an [`Effect`] uses to hand back what it produced.
struct Deliver<M> {
    send: Rc<dyn Fn(M)>,
    /// Closed when the effect is cancelled, so that what arrives later is discarded.
    open: Rc<Cell<bool>>,
}

impl<M> Deliver<M> {
    fn send(&self, message: M) {
        if self.open.get() {
            (self.send)(message);
        }
    }

    fn fail(&self, failure: &str) {
        if self.open.get() {
            report(ReportKind::Failure, failure);
        }
    }
}

/// What is left to do to stop an effect that has started.
enum Handle {
    Task(Task),
    Timer(Timer),
    /// A channel. The receiving side goes away once every [`Sender`] has been dropped
    /// and what they sent has arrived, which is what the `Weak` tells. Work on another
    /// thread cannot be stopped, so stopping closes the channel: what is waiting is
    /// dropped, and from then on sending fails, which the work can notice.
    Channel(Weak<()>, ChannelCloser),
}

/// An effect that has started. The model that owns it holds it, and so does each key
/// it was started under, which can be in a model further out.
struct Running {
    open: Rc<Cell<bool>>,
    handle: RefCell<Option<Handle>>,
}

impl Running {
    fn is_finished(&self) -> bool {
        !self.open.get()
            || match &*self.handle.borrow() {
                Some(Handle::Task(task)) => task.is_finished(),
                Some(Handle::Timer(timer)) => !timer.is_active(),
                Some(Handle::Channel(alive, _)) => alive.strong_count() == 0,
                None => false,
            }
    }

    fn cancel(&self) {
        self.open.set(false);
        // 借りたまま止めない。Future を捨てるとアプリのコードが走ることがある。
        let handle = self.handle.borrow_mut().take();
        match handle {
            Some(Handle::Task(task)) => task.cancel(),
            Some(Handle::Timer(timer)) => timer.cancel(),
            Some(Handle::Channel(_, closer)) => closer.close(),
            None => {}
        }
    }
}

struct Entry {
    /// `None` for the entry of the model that owns the effect. The ones under a key
    /// are there to be replaced.
    key: Option<Rc<str>>,
    running: Rc<Running>,
}

/// The effects one model has started.
#[derive(Default)]
struct Ledger {
    entries: RefCell<Vec<Entry>>,
}

impl Ledger {
    fn add(&self, key: Option<Rc<str>>, running: Rc<Running>) {
        let mut entries = self.entries.borrow_mut();
        // 終わったものをここで捨てるので、効果を起こし続けても増えていかない。
        entries.retain(|entry| !entry.running.is_finished());
        entries.push(Entry { key, running });
    }

    /// Cancels what `matches` selects. The ledger is not borrowed meanwhile, since
    /// dropping a future runs the app's code, which can reach this one.
    fn cancel(&self, matches: impl Fn(&Entry) -> bool) {
        let cancelled: Vec<_> = {
            let mut entries = self.entries.borrow_mut();
            let (cancelled, kept) = std::mem::take(&mut *entries).into_iter().partition(matches);
            *entries = kept;
            cancelled
        };
        for entry in cancelled {
            entry.running.cancel();
        }
    }

    fn running(&self) -> usize {
        self.entries
            .borrow()
            .iter()
            .filter(|entry| entry.key.is_none() && !entry.running.is_finished())
            .count()
    }
}

/// Owns a [`Ledger`]: dropping it cancels what is in there.
#[derive(Default)]
pub(crate) struct Owner(Rc<Ledger>);

impl Owner {
    pub(crate) fn running(&self) -> usize {
        self.0.running()
    }

    pub(crate) fn cancel_all(&self) {
        self.0.cancel(|_| true);
    }

    /// How many entries the ledger holds, finished ones included.
    #[cfg(test)]
    pub(crate) fn entries(&self) -> usize {
        self.0.entries.borrow().len()
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

/// What kind of work an effect starts, for [`Debug`](std::fmt::Debug).
enum Kind {
    Spawn,
    Thread,
    Channel,
    After(Duration),
    Every(Duration),
}

impl std::fmt::Debug for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Kind::Spawn => f.write_str("Spawn"),
            Kind::Thread => f.write_str("Thread"),
            Kind::Channel => f.write_str("Channel"),
            Kind::After(delay) => f.debug_tuple("After").field(delay).finish(),
            Kind::Every(interval) => f.debug_tuple("Every").field(interval).finish(),
        }
    }
}

/// Starts the work on the UI thread.
type Start<M> = Box<dyn FnOnce(&Tasks, Deliver<M>) -> Handle>;

struct Work<M> {
    kind: Kind,
    start: Start<M>,
}

enum Node<M> {
    Batch(Vec<Node<M>>),
    Work(Work<M>),
    Fail(String),
    /// Replaces what runs under the key in the model it reaches, with what is inside.
    Keyed(Rc<str>, Box<Node<M>>),
    /// What is inside belongs to this model, whichever model it passes through on the
    /// way to the app.
    Owned(Weak<Ledger>, Box<Node<M>>),
}

impl<M: 'static> Node<M> {
    fn map<N: 'static>(self, wrap: &Rc<dyn Fn(M) -> N>) -> Node<N> {
        match self {
            Node::Batch(nodes) => Node::Batch(nodes.into_iter().map(|n| n.map(wrap)).collect()),
            Node::Work(Work { kind, start }) => {
                let wrap = wrap.clone();
                Node::Work(Work {
                    kind,
                    start: Box::new(move |tasks, deliver: Deliver<N>| {
                        let open = deliver.open.clone();
                        let send = Rc::new(move |message| (deliver.send)(wrap(message)));
                        start(tasks, Deliver { send, open })
                    }),
                })
            }
            Node::Fail(failure) => Node::Fail(failure),
            Node::Keyed(key, node) => Node::Keyed(key, Box::new(node.map(wrap))),
            Node::Owned(owner, node) => Node::Owned(owner, Box::new(node.map(wrap))),
        }
    }

    fn start(
        self,
        cx: &mut Starting<'_, M>,
        ledger: Option<Rc<Ledger>>,
        keys: &mut Vec<(Rc<Ledger>, Rc<str>)>,
    ) {
        match self {
            Node::Batch(nodes) => {
                for node in nodes {
                    node.start(cx, ledger.clone(), keys);
                }
            }
            // 持ち主がいなくなっていても、起きた失敗はなかったことにしない。
            Node::Fail(failure) => report(ReportKind::Failure, &failure),
            Node::Owned(owner, node) => node.start(cx, owner.upgrade(), keys),
            Node::Keyed(key, node) => {
                let Some(owner) = &ledger else {
                    node.start(cx, None, keys);
                    return;
                };
                owner.cancel(|entry| entry.key.as_ref() == Some(&key));
                keys.push((owner.clone(), key));
                node.start(cx, ledger, keys);
                keys.pop();
            }
            Node::Work(work) => {
                // 持ち主はこの効果が始まる前に落とされた。
                let Some(ledger) = ledger else {
                    return;
                };
                let Some(tasks) = cx.tasks else {
                    report(
                        ReportKind::Failure,
                        "an effect needs naui's Tasks to run, and this App has none: \
                         make it with App::with_tasks",
                    );
                    return;
                };
                let open = Rc::new(Cell::new(true));
                let dispatch = cx.dispatch.clone();
                let deliver = Deliver {
                    send: Rc::new(move |message| dispatch.send(message)),
                    open: open.clone(),
                };
                let Some(handle) = attempt(&mut cx.panicked, || (work.start)(tasks, deliver))
                else {
                    return;
                };
                let running = Rc::new(Running {
                    open,
                    handle: RefCell::new(Some(handle)),
                });
                ledger.add(None, running.clone());
                for (owner, key) in keys.iter() {
                    owner.add(Some(key.clone()), running.clone());
                }
            }
        }
    }
}

struct Starting<'a, M: 'static> {
    tasks: Option<&'a Tasks>,
    dispatch: &'a Dispatch<M>,
    panicked: Option<Box<dyn Any + Send>>,
}

impl<M> std::fmt::Debug for Node<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Node::Batch(nodes) if nodes.is_empty() => f.write_str("None"),
            Node::Batch(nodes) => f.debug_list().entries(nodes).finish(),
            Node::Work(work) => work.kind.fmt(f),
            Node::Fail(failure) => f.debug_tuple("Fail").field(failure).finish(),
            Node::Keyed(key, node) => f.debug_tuple("Key").field(key).field(node).finish(),
            Node::Owned(_, node) => node.fmt(f),
        }
    }
}

/// What `update` asks for besides changing the model: asynchronous work whose results
/// come back as messages, and failures to report.
///
/// It is a value, and it does nothing by itself. [`Program::update`] returns it, and
/// the app starts it after the next `render`, once the borrow of the model has ended.
/// That keeps `update` plain Rust, which a test can call without a window and whose
/// result it can look at ([`is_none`](Self::is_none), [`keys`](Self::keys),
/// [`failures`](Self::failures), or the [`Debug`](std::fmt::Debug) output).
///
/// ```no_run
/// use std::time::Duration;
/// use naui_app::Effect;
///
/// enum Message {
///     QueryChanged(String),
///     Search,
///     Results(Result<Vec<String>, String>),
/// }
///
/// # fn search(_: &str) -> Result<Vec<String>, String> { Ok(Vec::new()) }
/// # struct Model { query: String, results: Vec<String> }
/// # impl Model {
/// fn update(&mut self, message: Message) -> Effect<Message> {
///     match message {
///         // Each key press replaces the wait of the one before, and the search it
///         // may have started, whose result is then discarded.
///         Message::QueryChanged(query) => {
///             self.query = query;
///             Effect::after(Duration::from_millis(300), Message::Search).key("search")
///         }
///         Message::Search => {
///             let query = self.query.clone();
///             Effect::thread(move || Message::Results(search(&query))).key("search")
///         }
///         Message::Results(Ok(results)) => {
///             self.results = results;
///             Effect::none()
///         }
///         Message::Results(Err(error)) => Effect::fail(error),
///     }
/// }
/// # }
/// ```
///
/// ## Who owns it
///
/// An effect belongs to the model whose `update` returned it, which stops it by being
/// dropped: the app for its own model, a [`Scoped`] for a child's. A task or a timer
/// is cancelled. Work on another thread cannot be stopped, so its result is discarded.
///
/// ## Failures
///
/// Work that can fail gives its result to a message, as `Results` does above, and the
/// `update` that receives it can both change the model and return
/// [`Effect::fail`]. So an error inside `update` and a failed effect take the same
/// path, and the model does not stay "loading" after a failure.
#[must_use = "an effect does nothing unless `update` returns it"]
pub struct Effect<M: 'static> {
    node: Node<M>,
}

impl<M: 'static> Effect<M> {
    /// Nothing to do.
    pub fn none() -> Self {
        Self {
            node: Node::Batch(Vec::new()),
        }
    }

    /// Runs a future on the UI thread, with [`Tasks::spawn`], and sends the message it
    /// ends with.
    ///
    /// `make` makes the future when the effect starts, and receives the [`Tasks`] to
    /// wait with ([`Tasks::sleep`]). As with `Tasks::spawn`, the future need not be
    /// `Send`, and blocking inside it freezes the screen. To send several messages (or
    /// to use an async runtime of your own), use [`channel`](Self::channel).
    ///
    /// Cancelling it drops the future.
    pub fn spawn<F>(make: impl FnOnce(Tasks) -> F + 'static) -> Self
    where
        F: Future<Output = M> + 'static,
    {
        Self::work(Kind::Spawn, move |tasks, deliver| {
            let future = make(tasks.clone());
            Handle::Task(tasks.spawn(async move {
                deliver.send(future.await);
            }))
        })
    }

    /// Runs `work` on a new thread, and sends the message it returns.
    ///
    /// The work cannot be stopped: when the effect is cancelled, it runs to its end
    /// and its message is dropped. If it panics, or a thread cannot be started, that
    /// is reported as a failure. On the Web there are no threads, so it always is.
    ///
    /// In those two cases no message arrives, so a model waiting for one stays as it
    /// was. An error the work can expect belongs in its message instead, as a `Result`.
    pub fn thread(work: impl FnOnce() -> M + Send + 'static) -> Self
    where
        M: Send,
    {
        Self::work(Kind::Thread, move |tasks, deliver| {
            let alive = Rc::new(());
            let closed = Rc::downgrade(&alive);
            let sender = tasks.channel(move |result: std::thread::Result<M>| {
                let _ = &alive;
                match result {
                    Ok(message) => deliver.send(message),
                    Err(panic) => deliver.fail(&format!(
                        "an effect's thread panicked: {}",
                        panic_text(panic.as_ref())
                    )),
                }
            });
            let closer = sender.closer();
            let started = std::thread::Builder::new()
                .name("naui-app effect".to_owned())
                .spawn(move || {
                    let _ = sender.send(catch_unwind(AssertUnwindSafe(work)));
                });
            if let Err(error) = started {
                report(
                    ReportKind::Failure,
                    &format!("could not start an effect's thread: {error}"),
                );
            }
            Handle::Channel(closed, closer)
        })
    }

    /// Opens a channel into the queue, with [`Tasks::channel`], and hands its [`Sender`]
    /// to `start`, which runs on the UI thread when the effect starts. Everything sent
    /// through it arrives as a message, in order.
    ///
    /// This is for work that sends more than once (progress), and for work an app runs
    /// itself, such as in a tokio runtime or a thread pool.
    ///
    /// When the effect is cancelled the channel is closed: what is waiting is dropped,
    /// and from then on [`Sender::send`] returns `Err` and [`Sender::is_open`] returns
    /// `false`, so work that keeps sending can tell, and stop. The same goes if `start`
    /// panics, since nothing could cancel the effect after that. Otherwise the channel
    /// closes once every `Sender` has been dropped.
    pub fn channel(start: impl FnOnce(Sender<M>) + 'static) -> Self
    where
        M: Send,
    {
        Self::work(Kind::Channel, move |tasks, deliver| {
            let alive = Rc::new(());
            let closed = Rc::downgrade(&alive);
            let sender = tasks.channel(move |message| {
                let _ = &alive;
                deliver.send(message);
            });
            let closer = sender.closer();
            // start が Sender をよそへ渡してから panic しても、そこから届かないように閉じる。
            // 帳簿に載らないので、あとから止める手段がない。
            if let Err(panic) = catch_unwind(AssertUnwindSafe(|| start(sender))) {
                closer.close();
                resume_unwind(panic);
            }
            Handle::Channel(closed, closer)
        })
    }

    /// Sends `message` once, after `delay` ([`Tasks::after`]).
    pub fn after(delay: Duration, message: M) -> Self {
        Self::work(Kind::After(delay), move |tasks, deliver| {
            Handle::Timer(tasks.after(delay, move || deliver.send(message)))
        })
    }

    /// Sends the message `make` returns every `interval` ([`Tasks::every`]), the first
    /// time after `interval` too, until it is cancelled. Give it a [`key`](Self::key)
    /// to be able to [`cancel`](Self::cancel) it while the model is still there.
    pub fn every(interval: Duration, mut make: impl FnMut() -> M + 'static) -> Self {
        Self::work(Kind::Every(interval), move |tasks, deliver| {
            Handle::Timer(tasks.every(interval, move || deliver.send(make())))
        })
    }

    /// Reports a failure to the app's handler
    /// ([`App::set_error_handler`](crate::App::set_error_handler)), or without one to
    /// standard error.
    ///
    /// It is reported when the effect would start, after the next `render`. For an
    /// error inside `update`, and for a failed effect, whose result comes back to
    /// `update` as a message. The report carries the message `update` was given.
    pub fn fail(failure: impl std::fmt::Display) -> Self {
        Self {
            node: Node::Fail(failure.to_string()),
        }
    }

    /// Cancels what runs under `key` in this model, as an effect under the same key
    /// would, with nothing to replace it.
    pub fn cancel(key: impl Into<Rc<str>>) -> Self {
        Self::none().key(key)
    }

    /// All of `effects`, started in order.
    pub fn batch(effects: impl IntoIterator<Item = Self>) -> Self {
        let mut nodes: Vec<_> = effects
            .into_iter()
            .filter(|effect| !effect.is_none())
            .map(|effect| effect.node)
            .collect();
        let node = if nodes.len() == 1 {
            nodes.remove(0)
        } else {
            Node::Batch(nodes)
        };
        Self { node }
    }

    /// This effect, then `other`.
    pub fn and(self, other: Self) -> Self {
        Self::batch([self, other])
    }

    /// Puts this effect under `key`: when it starts, it replaces what runs under the
    /// same key in this model. A task or a timer there is cancelled, and the result of
    /// work on another thread is discarded (a search overtaken by further typing).
    ///
    /// Keys belong to a model, so children in [`Scoped`] wrappers can use the same
    /// ones. A child without a wrapper shares its parent's.
    pub fn key(self, key: impl Into<Rc<str>>) -> Self {
        Self {
            node: Node::Keyed(key.into(), Box::new(self.node)),
        }
    }

    /// Turns a child's effect into its parent's, wrapping its messages. For example
    /// `self.editor.update(message).map(Message::Editor)`.
    ///
    /// Whom the effect belongs to does not change: a child's stays its [`Scoped`]
    /// wrapper's.
    pub fn map<N: 'static>(self, wrap: impl Fn(M) -> N + 'static) -> Effect<N> {
        let wrap: Rc<dyn Fn(M) -> N> = Rc::new(wrap);
        Effect {
            node: self.node.map(&wrap),
        }
    }

    /// Whether this asks for nothing at all.
    pub fn is_none(&self) -> bool {
        fn is_empty<M>(node: &Node<M>) -> bool {
            match node {
                Node::Batch(nodes) => nodes.iter().all(is_empty),
                Node::Owned(_, node) => is_empty(node),
                Node::Work(_) | Node::Fail(_) | Node::Keyed(..) => false,
            }
        }
        is_empty(&self.node)
    }

    /// The keys in this effect, in the order they would be applied.
    pub fn keys(&self) -> Vec<&str> {
        fn collect<'a, M>(node: &'a Node<M>, keys: &mut Vec<&'a str>) {
            match node {
                Node::Batch(nodes) => nodes.iter().for_each(|node| collect(node, keys)),
                Node::Keyed(key, node) => {
                    keys.push(key);
                    collect(node, keys);
                }
                Node::Owned(_, node) => collect(node, keys),
                Node::Work(_) | Node::Fail(_) => {}
            }
        }
        let mut keys = Vec::new();
        collect(&self.node, &mut keys);
        keys
    }

    /// The failures this effect reports, in order.
    pub fn failures(&self) -> Vec<&str> {
        fn collect<'a, M>(node: &'a Node<M>, failures: &mut Vec<&'a str>) {
            match node {
                Node::Batch(nodes) => nodes.iter().for_each(|node| collect(node, failures)),
                Node::Fail(failure) => failures.push(failure),
                Node::Keyed(_, node) | Node::Owned(_, node) => collect(node, failures),
                Node::Work(_) => {}
            }
        }
        let mut failures = Vec::new();
        collect(&self.node, &mut failures);
        failures
    }

    fn work(kind: Kind, start: impl FnOnce(&Tasks, Deliver<M>) -> Handle + 'static) -> Self {
        Self {
            node: Node::Work(Work {
                kind,
                start: Box::new(start),
            }),
        }
    }

    /// Hands what is in here to `owner`, unless a model further in already has it.
    fn owned_by(self, owner: &Owner) -> Self {
        Self {
            node: Node::Owned(Rc::downgrade(&owner.0), Box::new(self.node)),
        }
    }

    /// Starts what is in here, as `owner`'s. A panic in the code it calls does not stop
    /// the rest, and carries on once they have started.
    pub(crate) fn start(self, tasks: Option<&Tasks>, dispatch: &Dispatch<M>, owner: &Owner) {
        let mut cx = Starting {
            tasks,
            dispatch,
            panicked: None,
        };
        self.node
            .start(&mut cx, Some(owner.0.clone()), &mut Vec::new());
        if let Some(panic) = cx.panicked {
            resume_unwind(panic);
        }
    }
}

impl<M: 'static> Default for Effect<M> {
    fn default() -> Self {
        Self::none()
    }
}

impl<M: 'static> std::fmt::Debug for Effect<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.node.fmt(f)
    }
}

/// A child's model, together with the effects its `update` started: dropping this
/// cancels them.
///
/// The parent keeps it in its model where it would keep the child, as a field or in a
/// collection. It reads and changes like the child (it dereferences to it), and its
/// [`update`](Self::update) marks the effects it returns as the child's. So removing a
/// child from the collection in the parent's `update` is all it takes: a task or a
/// timer is cancelled, and the result of work on another thread is discarded. An
/// effect that has not started yet does not start. A task never outlives the state its
/// result is for, and all this needs nothing from the view, whose widgets follow in
/// the next `render`.
///
/// ```no_run
/// # use naui_app::{Effect, Program, Scoped};
/// # struct Row { id: u64 }
/// # impl Program for Row {
/// #     type Message = ();
/// #     type View = ();
/// #     fn update(&mut self, _: ()) -> Effect<()> { Effect::none() }
/// #     fn render(&self, _: &()) {}
/// # }
/// # enum Message { Row(u64, ()), Remove(u64) }
/// # struct Model { rows: Vec<Scoped<Row>> }
/// # impl Model {
/// fn update(&mut self, message: Message) -> Effect<Message> {
///     match message {
///         Message::Remove(id) => {
///             // What these rows started stops here.
///             self.rows.retain(|row| row.id != id);
///             Effect::none()
///         }
///         Message::Row(id, message) => match self.rows.iter_mut().find(|row| row.id == id) {
///             Some(row) => row.update(message).map(move |message| Message::Row(id, message)),
///             None => Effect::none(),
///         },
///     }
/// }
/// # }
/// ```
///
/// A child that lives as long as its parent, or that starts nothing, needs no wrapper:
/// what it starts then belongs to the parent, along with its keys.
pub struct Scoped<C: Program> {
    model: C,
    owner: Owner,
}

impl<C: Program> Scoped<C> {
    /// Keeps `model`, which has started nothing yet.
    pub fn new(model: C) -> Self {
        Self {
            model,
            owner: Owner::default(),
        }
    }

    /// Hands `message` to the child's `update`, and marks the effect it returns as the
    /// child's.
    pub fn update(&mut self, message: C::Message) -> Effect<C::Message> {
        self.model.update(message).owned_by(&self.owner)
    }

    /// How many of the child's effects have started and not finished.
    pub fn running(&self) -> usize {
        self.owner.running()
    }
}

impl<C: Program> Deref for Scoped<C> {
    type Target = C;

    fn deref(&self) -> &C {
        &self.model
    }
}

impl<C: Program> DerefMut for Scoped<C> {
    fn deref_mut(&mut self) -> &mut C {
        &mut self.model
    }
}

/// So that [`Children::sync`](crate::Children::sync) takes wrapped models as they are.
impl<C: Program> Borrow<C> for Scoped<C> {
    fn borrow(&self) -> &C {
        &self.model
    }
}

impl<C: Program + Default> Default for Scoped<C> {
    fn default() -> Self {
        Self::new(C::default())
    }
}

impl<C: Program> From<C> for Scoped<C> {
    fn from(model: C) -> Self {
        Self::new(model)
    }
}

impl<C: Program + std::fmt::Debug> std::fmt::Debug for Scoped<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scoped")
            .field("model", &self.model)
            .field("running", &self.running())
            .finish()
    }
}
