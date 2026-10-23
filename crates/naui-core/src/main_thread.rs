//! The foundation for queuing work onto the UI thread.
//!
//! The only thing a backend supplies is a single [`MainThread`]; both channels
//! ([`crate::channel`]) and tasks ([`crate::task`]) are built on top of it.
//!
//! The actual objects (receiving closures and futures) hold `Rc`s, so they live in
//! a `thread_local!` registry on the UI thread, and only `u64` ids cross threads.
//! Neither native widgets nor `!Send` futures ever leave the UI thread.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::channel::Sender;
use crate::task::Task;
use crate::{Error, Slot};

/// Work that runs exactly once on the UI thread.
pub type Work = Box<dyn FnOnce() + Send + 'static>;

/// The mechanism, supplied by a backend, for "running something once on the UI
/// thread".
///
/// **It must not run synchronously.** It must always defer, on every platform.
/// naui's promise that `send` and `spawn` never trigger a callback or poll on the
/// spot rests entirely on this.
///
/// Not something apps implement.
pub trait MainThread: Send + Sync + 'static {
    /// `true` if queued. `false` if it cannot be delivered, for example because the
    /// event loop has finished.
    fn post(&self, work: Work) -> bool;

    /// Runs once on the UI thread after waiting `delay`. `true` if queued.
    ///
    /// Leave the waiting to the event loop (never block a thread to wait). Even when
    /// `delay` is 0, defer it just like [`post`](MainThread::post).
    fn post_after(&self, delay: std::time::Duration, work: Work) -> bool;
}

/// Bundles where to post and "whether the UI is still alive".
///
/// `TryEnqueue` returns `Ok(false)` after shutdown, but the macOS main queue and
/// GTK idle sources do not report failure the same way. To make the meaning the
/// same on all four platforms, core checks `alive` first.
pub(crate) struct AppDispatch {
    alive: AtomicBool,
    main_thread: Arc<dyn MainThread>,
}

impl AppDispatch {
    fn new(main_thread: Arc<dyn MainThread>) -> Self {
        Self {
            alive: AtomicBool::new(true),
            main_thread,
        }
    }

    pub(crate) fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    fn shutdown(&self) {
        self.alive.store(false, Ordering::Release);
    }

    /// Queues work onto the UI thread. If already finished, does not queue and returns
    /// `false`.
    pub(crate) fn post(&self, work: Work) -> bool {
        if !self.is_alive() {
            return false;
        }
        self.main_thread.post(work)
    }

    /// Queues work onto the UI thread after a delay. If already finished, does not
    /// queue and returns `false`.
    pub(crate) fn post_after(&self, delay: std::time::Duration, work: Work) -> bool {
        if !self.is_alive() {
            return false;
        }
        self.main_thread.post_after(delay, work)
    }

    /// An error reporting that the UI has finished.
    pub(crate) fn closed(context: &'static str) -> Error {
        Error::new(context, "UI はもう受け取れません")
    }
}

/// Something that can be looked up again by id and woken on the UI thread.
///
/// `ChannelLocal<T>` has a different `T` per channel, so the type must be erased in
/// this form to put them in a single registry.
pub(crate) trait LocalEntry {
    fn pump(&self);
}

thread_local! {
    /// The registry of channels / tasks alive on the UI thread.
    static REGISTRY: RefCell<HashMap<u64, Rc<dyn LocalEntry>>> =
        RefCell::new(HashMap::new());
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
}

pub(crate) fn next_id() -> u64 {
    NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id.wrapping_add(1));
        id
    })
}

pub(crate) fn register(id: u64, entry: Rc<dyn LocalEntry>) {
    let _ = REGISTRY.try_with(|r| r.borrow_mut().insert(id, entry));
}

/// Removes an entry from the registry. The `Rc` taken out is dropped only after the
/// borrow is released (so the registry does not break if it is touched during the
/// drop).
pub(crate) fn deregister(id: u64) {
    let removed = REGISTRY.try_with(|r| r.borrow_mut().remove(&id));
    drop(removed);
}

/// Looks up the object by id and wakes it.
///
/// **Never call a callback while holding a borrow of the registry.** Re-entrancy,
/// such as opening or closing another channel from inside the callback, would make
/// the `RefCell` panic, so the `Rc` is cloned and the borrow released before the
/// call.
pub(crate) fn pump_by_id(id: u64) {
    let entry = REGISTRY
        .try_with(|r| r.borrow().get(&id).cloned())
        .ok()
        .flatten();
    if let Some(entry) = entry {
        entry.pump();
    }
}

fn clear_registry() {
    let entries = REGISTRY.try_with(|r| std::mem::take(&mut *r.borrow_mut()));
    drop(entries);
}

/// Takes out the closure, calls it, and puts it back unless it was replaced.
///
/// A common idiom in naui to avoid double borrows when the same widget is touched
/// during the call. Panics are stopped here (no backend can unwind across the C /
/// WinRT boundary). The default panic hook prints the panic itself to stderr, so
/// nothing is printed here.
pub(crate) fn call_guarded<T>(slot: &RefCell<Slot<dyn FnMut(T)>>, value: T) {
    let taken = slot.borrow_mut().take();
    let Some(mut f) = taken else {
        return;
    };
    let _ = catch_unwind(AssertUnwindSafe(|| f(value)));
    let mut slot = slot.borrow_mut();
    if slot.is_none() {
        *slot = Some(f);
    }
}

/// A handle for queuing work that runs on the UI thread.
///
/// Created with [`Ui::tasks`](../../naui/struct.Ui.html#method.tasks). It can be
/// cloned and moved into callbacks, but **cannot be sent to other threads**
/// (`!Send`).
///
/// Use [`channel`](Tasks::channel) to bring results from other threads back to the
/// screen, and [`spawn`](Tasks::spawn) to run futures on the UI thread.
#[derive(Clone)]
pub struct Tasks {
    dispatch: Arc<AppDispatch>,
    /// Holding an `Rc` makes this `!Send` (with only an `Arc` it would be `Send`).
    _local: Rc<()>,
}

impl Tasks {
    /// Called by backends from `Ui::new`. Not for use by apps.
    #[doc(hidden)]
    pub fn from_main_thread(main_thread: Arc<dyn MainThread>) -> Self {
        Self {
            dispatch: Arc::new(AppDispatch::new(main_thread)),
            _local: Rc::new(()),
        }
    }

    /// Reports that the UI runtime has finished. Called from a backend's shutdown.
    ///
    /// After this, [`Sender::send`] fails, and receiving closures and futures still in
    /// the registry are released. Not for use by apps.
    #[doc(hidden)]
    pub fn shutdown(&self) {
        self.dispatch.shutdown();
        clear_registry();
    }

    /// Opens a channel that receives values from other threads.
    ///
    /// `on_value` is called **on the UI thread**, so it may touch widgets. The returned
    /// [`Sender`] is `Send + Sync + Clone`, so it can be handed to as many threads as
    /// you like.
    ///
    /// Values arrive **in the order they were sent** (with several channels, the order
    /// across channels is unspecified). [`send`](Sender::send) does not call the
    /// closure on the spot; the value arrives once the UI thread is free.
    pub fn channel<T, F>(&self, on_value: F) -> Sender<T>
    where
        T: Send + 'static,
        F: FnMut(T) + 'static,
    {
        crate::channel::open(&self.dispatch, on_value)
    }

    /// Runs a future on the UI thread.
    ///
    /// The future need not be `Send`, so it can hold widget handles. It is driven by the
    /// UI thread's event loop, so **blocking inside it freezes the screen**. Move heavy
    /// work out through a [`channel`](Tasks::channel).
    ///
    /// The task keeps running even if the return value is dropped. Use
    /// [`Task::cancel`] to stop it.
    pub fn spawn<F>(&self, future: F) -> Task
    where
        F: std::future::Future<Output = ()> + 'static,
    {
        crate::task::spawn(&self.dispatch, future)
    }

    /// Calls `f` once on the UI thread after `delay` has passed.
    ///
    /// The timer does not stop when the return value is dropped. Use
    /// [`Timer::cancel`] to stop it. To search once typing pauses (debouncing),
    /// `cancel` the previous timer and create a new one on every keystroke.
    ///
    /// [`Timer::cancel`]: crate::Timer::cancel
    pub fn after(&self, delay: std::time::Duration, f: impl FnOnce() + 'static) -> crate::Timer {
        let mut f = Some(f);
        crate::timer::start(
            &self.dispatch,
            delay,
            None,
            Box::new(move || {
                if let Some(f) = f.take() {
                    f();
                }
            }),
        )
    }

    /// Calls `f` on the UI thread every `interval`. The first call also comes after
    /// `interval`.
    ///
    /// The timer does not stop when the return value is dropped. Use
    /// [`Timer::cancel`](crate::Timer::cancel) to stop it (calling it from inside the
    /// callback is fine). The next call is counted from when the previous call
    /// finishes, so calls never overlap even with heavy work.
    pub fn every(&self, interval: std::time::Duration, f: impl FnMut() + 'static) -> crate::Timer {
        crate::timer::start(&self.dispatch, interval, Some(interval), Box::new(f))
    }

    /// A future that completes after `delay`. Use it to wait inside
    /// [`spawn`](Tasks::spawn).
    ///
    /// ```ignore
    /// let tasks = ui.tasks();
    /// tasks.clone().spawn(async move {
    ///     for attempt in 1.. {
    ///         if connect().await.is_ok() { break; }
    ///         tasks.sleep(Duration::from_secs(attempt.min(30))).await; // wait before reconnecting
    ///     }
    /// });
    /// ```
    pub fn sleep(&self, delay: std::time::Duration) -> crate::Sleep {
        crate::timer::Sleep::new(&self.dispatch, delay)
    }
}

impl std::fmt::Debug for Tasks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tasks")
            .field("alive", &self.dispatch.is_alive())
            .finish()
    }
}
