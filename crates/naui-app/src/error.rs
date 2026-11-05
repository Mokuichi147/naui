//! One handler for what goes wrong: panics on the UI thread, failures, children that
//! could not be mounted and repeated keys.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::fmt::{self, Debug, Write};
use std::panic::{catch_unwind, set_hook, take_hook, AssertUnwindSafe, PanicHookInfo};
use std::rc::{Rc, Weak};
use std::sync::Once;

/// How long the text of a message can get, in bytes.
const MESSAGE_TEXT_LIMIT: usize = 1024;

/// Whether a panic can be caught at all. On wasm and with `panic = "abort"` it cannot.
const CAN_UNWIND: bool = cfg!(panic = "unwind");

/// What kind of thing went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReportKind {
    /// A panic on the UI thread, in the queue or in any other naui callback.
    Panic,
    /// A failure: [`Effect::fail`](crate::Effect::fail), an effect's thread that
    /// panicked or could not start, or work asked of an app without [`Tasks`](naui::Tasks).
    Failure,
    /// A child [`Children::sync`](crate::Children::sync) could not mount. It tries
    /// again on every call, and this comes once until it works or the key goes away.
    Mount,
    /// A key given to [`Children::sync`](crate::Children::sync) more than once. It
    /// comes once until the keys are unique again.
    RepeatedKey,
}

/// What the handler is told. See [`App::set_error_handler`](crate::App::set_error_handler).
#[derive(Debug, Clone)]
pub struct Report {
    kind: ReportKind,
    text: String,
    location: Option<String>,
    message: Option<Rc<str>>,
}

impl Report {
    /// What kind of thing went wrong.
    pub fn kind(&self) -> ReportKind {
        self.kind
    }

    /// What went wrong: a panic's text, or the failure as it was reported.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Where a panic happened (`file:line:column`), when that is known. A panic resumed
    /// with [`resume_unwind`](std::panic::resume_unwind) does not say.
    pub fn location(&self) -> Option<&str> {
        self.location.as_deref()
    }

    /// The [`Debug`] text of the message the queue was handling, cut to 1024 bytes.
    ///
    /// - From `update`: the message it was given.
    /// - From `render`: the last message handled before it, of the several that may
    ///   have led to it. The first `render` has none.
    /// - From an effect starting (a [`fail`](crate::Effect::fail) among them): the
    ///   message whose `update` returned the effect.
    /// - From outside the queue (an ordinary naui callback, a thread that ended
    ///   later): none.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.kind, &self.location) {
            (ReportKind::Panic, Some(location)) => write!(f, "panic at {location}: ")?,
            (ReportKind::Panic, None) => f.write_str("panic: ")?,
            _ => {}
        }
        f.write_str(&self.text)?;
        if let Some(message) = &self.message {
            write!(f, " (while handling {message})")?;
        }
        Ok(())
    }
}

/// What the handler wants done after a report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Carry on. After a panic, the model is used as the panic left it.
    GoOn,
    /// Stop the app: no more messages are handled, an effect that has not started does
    /// not start (the rest of the same [`batch`](crate::Effect::batch) included), the
    /// effects it has started are cancelled, those of children in a
    /// [`Scoped`](crate::Scoped) too, and the UI quits (only [`run`] and
    /// [`run_with_handler`] know the UI).
    ///
    /// [`run`]: crate::run
    /// [`run_with_handler`]: crate::run_with_handler
    Stop,
}

type Handler = Box<dyn FnMut(&Report) -> Decision>;

/// The app's handler, and how to stop that app.
pub(crate) struct Slot {
    handler: RefCell<Handler>,
    stop: Box<dyn Fn()>,
}

impl Slot {
    pub(crate) fn new(handler: Handler, stop: Box<dyn Fn()>) -> Rc<Self> {
        Rc::new(Self {
            handler: RefCell::new(handler),
            stop,
        })
    }

    /// Asks the handler, unless it is the one that caused this.
    fn ask(&self, report: &Report) -> Option<Decision> {
        let mut handler = self.handler.try_borrow_mut().ok()?;
        TELLING.with(|telling| telling.set(true));
        // panic hook の中からの呼び出しでは受け止められず、プロセスが落ちる。
        let decision = catch_unwind(AssertUnwindSafe(|| handler(report))).unwrap_or_else(|_| {
            to_standard_error("the error handler panicked, so the app goes on");
            Decision::GoOn
        });
        TELLING.with(|telling| telling.set(false));
        Some(decision)
    }

    fn is_busy(&self) -> bool {
        self.handler.try_borrow_mut().is_err()
    }
}

/// A panic the hook saw, waiting for the queue to catch it.
struct Panicked {
    text: String,
    location: Option<String>,
}

thread_local! {
    /// The handlers of this thread's apps, oldest first, each owned by the app that set
    /// it. What cannot be tied to an app goes to the last one still there. Other threads
    /// have none, so the panics there only go on to the previous hook.
    static SLOTS: RefCell<Vec<Weak<Slot>>> = const { RefCell::new(Vec::new()) };
    /// The handler of the app whose queue is running, which is empty if that app has
    /// none. `None` outside any queue.
    static QUEUE: RefCell<Option<Weak<Slot>>> = const { RefCell::new(None) };
    /// How many of [`catch`]'s calls are running.
    static CATCHING: Cell<usize> = const { Cell::new(0) };
    /// Panics seen inside [`catch`].
    static PENDING: RefCell<Vec<Panicked>> = const { RefCell::new(Vec::new()) };
    /// The text of the message being handled.
    static CURRENT: RefCell<Option<Rc<str>>> = const { RefCell::new(None) };
    /// Whether the handler is being called.
    static TELLING: Cell<bool> = const { Cell::new(false) };
}

/// Whom a report goes to: the app whose queue is running, or else the last app that set
/// a handler.
fn current_slot() -> Option<Rc<Slot>> {
    let queue = QUEUE
        .try_with(|queue| queue.try_borrow().ok().and_then(|queue| queue.clone()))
        .ok()
        .flatten();
    if let Some(slot) = queue {
        return slot.upgrade();
    }
    SLOTS
        .try_with(|slots| {
            let slots = slots.try_borrow().ok()?;
            slots.iter().rev().find_map(Weak::upgrade)
        })
        .ok()
        .flatten()
}

fn current_message() -> Option<Rc<str>> {
    CURRENT
        .try_with(|current| current.try_borrow().ok().and_then(|text| text.clone()))
        .ok()
        .flatten()
}

/// Makes `slot` the handler for what cannot be tied to an app. The caller owns it: once
/// that lets go of it, the one before takes over.
pub(crate) fn set_slot(slot: &Rc<Slot>) {
    SLOTS.with(|slots| {
        let mut slots = slots.borrow_mut();
        slots.retain(|slot| slot.strong_count() > 0);
        slots.push(Rc::downgrade(slot));
    });
    install_hook();
}

/// Makes `slot` the handler of the queue that is running.
pub(crate) fn set_queue_slot(slot: &Rc<Slot>) {
    QUEUE.with(|queue| *queue.borrow_mut() = Some(Rc::downgrade(slot)));
}

/// Puts naui-app's panic hook in front of the one there, once.
fn install_hook() {
    static INSTALLED: Once = Once::new();
    // 巻き戻しの途中では hook を取り替えられない (take_hook が panic する)。
    if std::thread::panicking() {
        return;
    }
    INSTALLED.call_once(|| {
        let previous = take_hook();
        set_hook(Box::new(move |info| {
            on_panic(info);
            previous(info);
        }));
    });
}

fn on_panic(info: &PanicHookInfo<'_>) {
    let Some(slot) = current_slot() else {
        return;
    };
    // ハンドラ自身の panic はハンドラへ伝えない。内容は前の hook が出す。
    if slot.is_busy() {
        return;
    }
    let text = panic_text(info.payload()).to_owned();
    let location = info.location().map(ToString::to_string);
    if CAN_UNWIND && CATCHING.try_with(Cell::get).unwrap_or(0) > 0 {
        // 受け止めたところで伝える。モデルの借用が終わっていて、ハンドラの panic も
        // 受け止められるので、この中で呼ぶより安全。
        let _ = PENDING.try_with(|pending| {
            if let Ok(mut pending) = pending.try_borrow_mut() {
                pending.push(Panicked { text, location });
            }
        });
        return;
    }
    let decision = slot.ask(&Report {
        kind: ReportKind::Panic,
        text,
        location,
        message: current_message(),
    });
    // 巻き戻せないなら、ハンドラの答えによらずここで終わる。
    if !CAN_UNWIND || decision == Some(Decision::Stop) {
        (slot.stop)();
    }
}

/// Runs a step of the queue, catching a panic and telling the handler about it (and about
/// any other panic seen meanwhile, such as those of several children).
pub(crate) fn catch(step: impl FnOnce()) {
    // 外側の catch が伝える分には触らない。別のアプリのものかもしれない。
    let waiting = PENDING.with(|pending| pending.borrow().len());
    CATCHING.with(|depth| depth.set(depth.get() + 1));
    let outcome = catch_unwind(AssertUnwindSafe(step));
    CATCHING.with(|depth| depth.set(depth.get() - 1));
    let mut panics = PENDING.with(|pending| pending.borrow_mut().split_off(waiting));
    if current_slot().is_none() {
        return;
    }
    if let (Err(panic), true) = (&outcome, panics.is_empty()) {
        // resume_unwind は hook を通らない。
        panics.push(Panicked {
            text: panic_text(panic.as_ref()).to_owned(),
            location: None,
        });
    }
    for panic in panics {
        deliver(&Report {
            kind: ReportKind::Panic,
            text: panic.text,
            location: panic.location,
            message: current_message(),
        });
    }
}

/// Reports what went wrong: to the handler, or without one to standard error.
pub(crate) fn report(kind: ReportKind, text: &str) {
    deliver(&Report {
        kind,
        text: text.to_owned(),
        location: None,
        message: current_message(),
    });
}

fn deliver(report: &Report) {
    let Some(slot) = current_slot() else {
        to_standard_error(report.text());
        return;
    };
    match slot.ask(report) {
        Some(Decision::GoOn) => {}
        Some(Decision::Stop) => (slot.stop)(),
        None => to_standard_error(&report.to_string()),
    }
}

fn to_standard_error(line: &str) {
    #[cfg(test)]
    tests_support::REPORTED.with(|reported| reported.borrow_mut().push(line.to_owned()));
    #[cfg(not(test))]
    eprintln!("naui-app: {line}");
}

/// Whether the queue has to wait instead of running here: during a panic another one
/// would abort the process, and the handler is being called (and is not to be called
/// again from inside).
pub(crate) fn must_wait() -> bool {
    std::thread::panicking() || TELLING.try_with(Cell::get).unwrap_or(false)
}

/// Sets the text of the message being handled.
pub(crate) fn set_current_message(text: Option<Rc<str>>) {
    CURRENT.with(|current| *current.borrow_mut() = text);
}

/// Starts with no message being handled, and puts back the one before when dropped.
pub(crate) struct MessageScope(Option<Rc<str>>);

impl MessageScope {
    pub(crate) fn enter() -> Self {
        Self(CURRENT.with(|current| current.borrow_mut().take()))
    }
}

impl Drop for MessageScope {
    fn drop(&mut self) {
        set_current_message(self.0.take());
    }
}

/// What runs meanwhile is `slot`'s app's, with no message being handled yet. Puts back
/// the app and the message before when dropped.
pub(crate) struct QueueScope {
    previous: Option<Weak<Slot>>,
    _message: MessageScope,
}

impl QueueScope {
    pub(crate) fn enter(slot: Weak<Slot>) -> Self {
        Self {
            previous: QUEUE.with(|queue| queue.borrow_mut().replace(slot)),
            _message: MessageScope::enter(),
        }
    }
}

impl Drop for QueueScope {
    fn drop(&mut self) {
        QUEUE.with(|queue| *queue.borrow_mut() = self.previous.take());
    }
}

/// Stops taking text once the limit is reached, so that formatting a large message ends
/// early.
struct Bounded {
    text: String,
    cut: bool,
}

impl Write for Bounded {
    fn write_str(&mut self, part: &str) -> fmt::Result {
        let left = MESSAGE_TEXT_LIMIT - self.text.len();
        if part.len() <= left {
            self.text.push_str(part);
            return Ok(());
        }
        let mut end = left;
        while !part.is_char_boundary(end) {
            end -= 1;
        }
        self.text.push_str(&part[..end]);
        self.cut = true;
        Err(fmt::Error)
    }
}

/// The text of a message, cut to [`MESSAGE_TEXT_LIMIT`] bytes.
pub(crate) fn describe<M: Debug>(message: &M) -> Rc<str> {
    let mut bounded = Bounded {
        text: String::new(),
        cut: false,
    };
    let _ = write!(bounded, "{message:?}");
    if bounded.cut {
        bounded.text.push('…');
    }
    bounded.text.into()
}

/// The text of a panic's payload, for the usual `panic!` forms.
pub(crate) fn panic_text(panic: &(dyn Any + Send)) -> &str {
    if let Some(text) = panic.downcast_ref::<&str>() {
        text
    } else if let Some(text) = panic.downcast_ref::<String>() {
        text
    } else {
        "(not a string)"
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    use std::cell::RefCell;

    thread_local! {
        /// What would have gone to standard error on this thread.
        pub(crate) static REPORTED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }
}
