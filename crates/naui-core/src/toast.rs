//! Value types for toasts (transient notifications), and the queue they wait in.
//!
//! A toast is a notification that "shows a short string and optional action buttons
//! for a while, then goes away by itself". Only libadwaita has a native one,
//! `AdwToast`. naui builds the same shape on the other three platforms, so **what
//! to show, for how long and where** lives here, like
//! [`NumberSpec`](crate::NumberSpec), and is shared by all four backends.
//!
//! One toast is on screen at a time and the rest wait their turn, the way
//! `AdwToastOverlay` queues them. That overlay keeps one queue per window and has no
//! signal for "a waiting toast came up", so the order is kept here, in
//! [`ToastQueue`], and all four backends, GTK included, put one toast at a time on
//! screen from it.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::thread::LocalKey;

use crate::Align;

/// How a shown toast takes its turn.
///
/// The same two levels as `AdwToastPriority`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ToastPriority {
    /// Waits behind the toasts already shown or waiting.
    #[default]
    Normal,
    /// Comes up at once. The toast on screen goes back to the front of the queue and
    /// comes up again, with its full time, once this one is gone.
    High,
}

/// Where in the window a toast appears.
///
/// `Start` and `End` are the sides where a row starts and ends: the left and the
/// right, swapped in a right-to-left layout
/// ([`LayoutDirection::Rtl`](crate::LayoutDirection::Rtl)).
///
/// `AdwToastOverlay` always places toasts at the bottom center and has no way to
/// change it, so **GTK ignores this**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ToastPosition {
    TopStart,
    TopCenter,
    TopEnd,
    BottomStart,
    /// Where `AdwToastOverlay` places them, and the default.
    #[default]
    BottomCenter,
    BottomEnd,
}

impl ToastPosition {
    /// All six, top row first, each row from start to end.
    pub const ALL: [ToastPosition; 6] = [
        ToastPosition::TopStart,
        ToastPosition::TopCenter,
        ToastPosition::TopEnd,
        ToastPosition::BottomStart,
        ToastPosition::BottomCenter,
        ToastPosition::BottomEnd,
    ];

    /// Whether it is along the top edge.
    ///
    /// ```
    /// # use naui_core::ToastPosition;
    /// assert!(ToastPosition::TopEnd.is_top());
    /// assert!(!ToastPosition::BottomCenter.is_top());
    /// ```
    pub fn is_top(self) -> bool {
        matches!(
            self,
            ToastPosition::TopStart | ToastPosition::TopCenter | ToastPosition::TopEnd
        )
    }

    /// Which way it sits along that edge: [`Align::Start`], [`Align::Center`] or
    /// [`Align::End`].
    ///
    /// ```
    /// # use naui_core::{Align, ToastPosition};
    /// assert_eq!(ToastPosition::TopStart.align(), Align::Start);
    /// assert_eq!(ToastPosition::BottomEnd.align(), Align::End);
    /// ```
    pub fn align(self) -> Align {
        match self {
            ToastPosition::TopStart | ToastPosition::BottomStart => Align::Start,
            ToastPosition::TopCenter | ToastPosition::BottomCenter => Align::Center,
            ToastPosition::TopEnd | ToastPosition::BottomEnd => Align::End,
        }
    }
}

/// What a toast shows, how long until it disappears, and how and where it comes up.
///
/// The default is "a toast at the bottom center that disappears after 5 seconds,
/// without action buttons, waiting its turn".
///
/// ```
/// # use naui_core::{ToastPosition, ToastPriority, ToastSpec};
/// let mut spec = ToastSpec::new("Saved");
/// assert_eq!(spec.timeout(), ToastSpec::DEFAULT_TIMEOUT);
/// assert!(spec.actions().is_empty());
///
/// spec.set_actions(&["Undo", "Show"]);
/// assert_eq!(spec.actions(), ["Undo", "Show"]);
///
/// spec.set_timeout(0.0); // 0 means "never disappears automatically"
/// assert!(spec.is_persistent());
///
/// assert_eq!(spec.priority(), ToastPriority::Normal);
/// assert_eq!(spec.position(), ToastPosition::BottomCenter);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ToastSpec {
    message: String,
    actions: Vec<String>,
    timeout: f64,
    priority: ToastPriority,
    position: ToastPosition,
}

impl Default for ToastSpec {
    fn default() -> Self {
        Self {
            message: String::new(),
            actions: Vec::new(),
            timeout: Self::DEFAULT_TIMEOUT,
            priority: ToastPriority::default(),
            position: ToastPosition::default(),
        }
    }
}

impl ToastSpec {
    /// Seconds until it disappears when nothing is specified. The same as `AdwToast`'s
    /// default.
    pub const DEFAULT_TIMEOUT: f64 = 5.0;

    /// The longest accepted duration in seconds (one day). Longer values are truncated
    /// to this.
    ///
    /// The cap exists because the browser's `setTimeout` accepts only 32-bit
    /// milliseconds. A toast shown for a whole day is effectively the same as
    /// [`is_persistent`](Self::is_persistent).
    pub const MAX_TIMEOUT: f64 = 86_400.0;

    /// Creates one with the string to show.
    pub fn new(message: &str) -> Self {
        Self {
            message: message.to_string(),
            ..Self::default()
        }
    }

    /// The string to show.
    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn set_message(&mut self, message: &str) {
        self.message = message.to_string();
    }

    /// The action buttons' labels, in the order they are laid out. Empty if there are
    /// none.
    pub fn actions(&self) -> &[String] {
        &self.actions
    }

    /// The action buttons' labels, laid out in this order from the start side. **An
    /// empty list removes the buttons.**
    ///
    /// The index of a label here is what `on_action` receives. Toasts are meant for
    /// one or two short actions; anything longer belongs in a `Dialog` or a `Sheet`.
    pub fn set_actions<S: AsRef<str>>(&mut self, labels: &[S]) {
        self.actions = labels.iter().map(|l| l.as_ref().to_string()).collect();
    }

    /// Seconds until it disappears. 0 means it never disappears automatically.
    pub fn timeout(&self) -> f64 {
        self.timeout
    }

    /// Seconds until it disappears, counted from when it comes up on screen (not from
    /// when it starts waiting).
    ///
    /// **Values of 0 or less, and values that are not numbers (`NaN` and infinities),
    /// are treated as 0.** 0 means "never disappears automatically": it goes away only
    /// through `dismiss()` or an action button (the same rule as `AdwToast`'s
    /// `timeout`). Values longer than [`MAX_TIMEOUT`](Self::MAX_TIMEOUT) are truncated
    /// to it.
    ///
    /// ```
    /// # use naui_core::ToastSpec;
    /// let mut spec = ToastSpec::new("Done");
    /// spec.set_timeout(-1.0);
    /// assert_eq!(spec.timeout(), 0.0, "negative values become \"never disappears\"");
    /// spec.set_timeout(f64::INFINITY);
    /// assert_eq!(spec.timeout(), 0.0);
    /// ```
    pub fn set_timeout(&mut self, seconds: f64) {
        self.timeout = if seconds.is_finite() && seconds > 0.0 {
            seconds.min(Self::MAX_TIMEOUT)
        } else {
            0.0
        };
    }

    /// Whether it never disappears automatically.
    pub fn is_persistent(&self) -> bool {
        self.timeout <= 0.0
    }

    /// How it takes its turn when shown.
    pub fn priority(&self) -> ToastPriority {
        self.priority
    }

    pub fn set_priority(&mut self, priority: ToastPriority) {
        self.priority = priority;
    }

    /// Where it appears.
    pub fn position(&self) -> ToastPosition {
        self.position
    }

    pub fn set_position(&mut self, position: ToastPosition) {
        self.position = position;
    }

    /// Milliseconds until it disappears. `None` if it never disappears automatically.
    ///
    /// Passed to the browser's `setTimeout` and WinUI's `DispatcherQueueTimer`.
    ///
    /// ```
    /// # use naui_core::ToastSpec;
    /// let mut spec = ToastSpec::new("Done");
    /// assert_eq!(spec.timeout_millis(), Some(5_000));
    /// spec.set_timeout(0.0);
    /// assert_eq!(spec.timeout_millis(), None);
    /// ```
    pub fn timeout_millis(&self) -> Option<i32> {
        if self.is_persistent() {
            return None;
        }
        // 上限が 1 日なので、ミリ秒にしても i32 に収まる。
        Some((self.timeout * 1_000.0).round().max(1.0) as i32)
    }

    /// Seconds until it disappears, as an integer. 0 if it never disappears
    /// automatically.
    ///
    /// `adw_toast_set_timeout` accepts only whole seconds, so **anything under one
    /// second becomes one second**. Rounding to 0 would turn it into a "toast that
    /// never disappears" on GTK.
    ///
    /// ```
    /// # use naui_core::ToastSpec;
    /// let mut spec = ToastSpec::new("Done");
    /// spec.set_timeout(0.2);
    /// assert_eq!(spec.timeout_secs(), 1);
    /// ```
    pub fn timeout_secs(&self) -> u32 {
        if self.is_persistent() {
            return 0;
        }
        self.timeout.round().max(1.0) as u32
    }
}

/// What a backend does to put one of its toasts on screen and take it off, for
/// [`ToastQueue`].
///
/// They are associated functions on the backend's shared state (`Rc<Self>`), so that
/// the queue can hold toasts without the backend's public `Toast` type exposing
/// them.
pub trait ToastSlot {
    /// Puts it on screen and starts counting down. Returns `false` if there is nowhere
    /// to put it (no window yet); the queue then drops it and goes on to the next.
    fn present(this: &Rc<Self>) -> bool;

    /// Takes it off screen and stops counting down, **without notifying the app**.
    fn withdraw(this: &Rc<Self>);

    /// Whether it is on screen in a window that has since closed. Such a toast would
    /// hold up the queue (for good, if it never disappears by itself), so the next
    /// [`ToastQueue::show`] takes it down without notifying, the same as `dismiss()`.
    fn is_stranded(_this: &Rc<Self>) -> bool {
        false
    }
}

/// One thing a backend does to its toasts.
enum Step<T> {
    Present(Rc<T>),
    Withdraw(Rc<T>),
}

/// The toast on screen and the ones waiting their turn.
///
/// Each backend keeps one in a `thread_local!` and goes through
/// [`show`](Self::show) and [`remove`](Self::remove), which decide what comes up
/// and what goes, then call the backend's [`ToastSlot`] functions **after letting
/// go of the queue**, so a backend may look at the queue (or show another toast)
/// from inside them.
///
/// The order follows `AdwToastOverlay`:
///
/// - With nothing on screen, a shown toast comes up at once.
/// - A [`Normal`](ToastPriority::Normal) toast otherwise waits at the back.
/// - A [`High`](ToastPriority::High) toast comes up at once, and the one on screen
///   goes back to the front of the queue.
/// - Showing the toast on screen again puts it up afresh (its time starts over).
///   Showing a waiting one again leaves it where it is, unless it is `High`.
/// - Removing the one on screen brings up the next; removing a waiting one only
///   takes it out of the queue.
#[derive(Debug)]
pub struct ToastQueue<T> {
    shown: Option<Rc<T>>,
    waiting: VecDeque<Rc<T>>,
}

impl<T> Default for ToastQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ToastQueue<T> {
    /// An empty queue. `const` so that it can start a `thread_local!`.
    pub const fn new() -> Self {
        Self {
            shown: None,
            waiting: VecDeque::new(),
        }
    }

    /// Whether `toast` is the one on screen.
    pub fn is_shown(&self, toast: &Rc<T>) -> bool {
        self.shown.as_ref().is_some_and(|s| Rc::ptr_eq(s, toast))
    }

    /// Whether `toast` is waiting its turn.
    pub fn is_waiting(&self, toast: &Rc<T>) -> bool {
        self.waiting.iter().any(|w| Rc::ptr_eq(w, toast))
    }

    /// The toast on screen.
    pub fn shown(&self) -> Option<Rc<T>> {
        self.shown.clone()
    }

    /// How many are waiting.
    pub fn waiting_len(&self) -> usize {
        self.waiting.len()
    }

    fn plan_show(&mut self, toast: &Rc<T>, priority: ToastPriority) -> Vec<Step<T>> {
        if self.is_shown(toast) {
            // 出ているものをもう一度: 出し直して時間を数え直す。
            return vec![Step::Withdraw(toast.clone()), Step::Present(toast.clone())];
        }
        if self.is_waiting(toast) {
            if priority == ToastPriority::Normal {
                return Vec::new(); // 並んだまま。
            }
            self.waiting.retain(|w| !Rc::ptr_eq(w, toast));
        }
        match (self.shown.take(), priority) {
            (None, _) => {
                self.shown = Some(toast.clone());
                vec![Step::Present(toast.clone())]
            }
            (Some(current), ToastPriority::Normal) => {
                self.shown = Some(current);
                self.waiting.push_back(toast.clone());
                Vec::new()
            }
            (Some(current), ToastPriority::High) => {
                self.waiting.push_front(current.clone());
                self.shown = Some(toast.clone());
                vec![Step::Withdraw(current), Step::Present(toast.clone())]
            }
        }
    }

    fn plan_remove(&mut self, toast: &Rc<T>) -> Vec<Step<T>> {
        if self.is_shown(toast) {
            self.shown = self.waiting.pop_front();
            let mut steps = vec![Step::Withdraw(toast.clone())];
            steps.extend(self.shown.clone().map(Step::Present));
            return steps;
        }
        self.waiting.retain(|w| !Rc::ptr_eq(w, toast));
        Vec::new()
    }
}

impl<T: ToastSlot> ToastQueue<T> {
    /// Shows `toast`: it comes up now or waits its turn, as described on the type.
    pub fn show(queue: &'static LocalKey<RefCell<Self>>, toast: &Rc<T>, priority: ToastPriority) {
        // 閉じた窓に取り残されたものは、並びを塞がないよう先に片付ける。
        if let Some(current) = queue.with(|q| q.borrow().shown()) {
            if !Rc::ptr_eq(&current, toast) && T::is_stranded(&current) {
                Self::remove(queue, &current);
            }
        }
        let steps = queue.with(|q| q.borrow_mut().plan_show(toast, priority));
        Self::run(queue, steps);
    }

    /// Takes `toast` out: off the screen (bringing up the next) or out of the queue.
    /// Does nothing if it is neither.
    ///
    /// The backend calls it both when the app dismisses a toast and when the toast
    /// goes by itself (time ran out, an action button was clicked); in the latter case
    /// it notifies the app afterwards, so the next toast is already up by then.
    pub fn remove(queue: &'static LocalKey<RefCell<Self>>, toast: &Rc<T>) {
        let steps = queue.with(|q| q.borrow_mut().plan_remove(toast));
        Self::run(queue, steps);
    }

    /// Whether `toast` is the one on screen.
    pub fn is_shown_in(queue: &'static LocalKey<RefCell<Self>>, toast: &Rc<T>) -> bool {
        queue.with(|q| q.borrow().is_shown(toast))
    }

    /// Whether `toast` is waiting its turn.
    pub fn is_waiting_in(queue: &'static LocalKey<RefCell<Self>>, toast: &Rc<T>) -> bool {
        queue.with(|q| q.borrow().is_waiting(toast))
    }

    fn run(queue: &'static LocalKey<RefCell<Self>>, steps: Vec<Step<T>>) {
        let mut steps = VecDeque::from(steps);
        while let Some(step) = steps.pop_front() {
            match step {
                Step::Withdraw(toast) => T::withdraw(&toast),
                Step::Present(toast) => {
                    // 前の手順の中で取り消されていたら出さない。
                    if !queue.with(|q| q.borrow().is_shown(&toast)) {
                        continue;
                    }
                    if !T::present(&toast) {
                        // 出す先が無い: 捨てて、次のものへ進む。
                        let more = queue.with(|q| q.borrow_mut().plan_remove(&toast));
                        // 出していないので、引っ込める手順は飛ばす。
                        steps.extend(
                            more.into_iter().filter(
                                |s| !matches!(s, Step::Withdraw(t) if Rc::ptr_eq(t, &toast)),
                            ),
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shows_a_plain_toast_for_five_seconds() {
        let spec = ToastSpec::default();
        assert_eq!(spec.message(), "");
        assert!(spec.actions().is_empty());
        assert_eq!(spec.timeout(), 5.0);
        assert!(!spec.is_persistent());
        assert_eq!(spec.priority(), ToastPriority::Normal);
        assert_eq!(spec.position(), ToastPosition::BottomCenter);
    }

    #[test]
    fn actions_keep_their_order_and_an_empty_list_removes_them() {
        let mut spec = ToastSpec::new("保存しました");
        spec.set_actions(&["元に戻す", "開く"]);
        assert_eq!(spec.actions(), ["元に戻す", "開く"]);
        spec.set_actions(&[String::from("やり直す")]);
        assert_eq!(spec.actions(), ["やり直す"]);
        spec.set_actions::<&str>(&[]);
        assert!(spec.actions().is_empty());
    }

    #[test]
    fn unusable_timeouts_become_persistent() {
        let mut spec = ToastSpec::new("完了");
        for seconds in [0.0, -3.0, f64::NAN, f64::NEG_INFINITY, f64::INFINITY] {
            spec.set_timeout(seconds);
            assert!(spec.is_persistent(), "{seconds} は「消えない」になる");
            assert_eq!(spec.timeout_millis(), None);
            assert_eq!(spec.timeout_secs(), 0);
        }
    }

    #[test]
    fn long_timeouts_are_cut_to_a_day() {
        let mut spec = ToastSpec::new("完了");
        spec.set_timeout(f64::MAX);
        assert_eq!(spec.timeout(), ToastSpec::MAX_TIMEOUT);
        assert_eq!(spec.timeout_millis(), Some(86_400_000));
    }

    #[test]
    fn short_timeouts_keep_at_least_one_unit() {
        let mut spec = ToastSpec::new("完了");
        spec.set_timeout(0.000_1);
        // ミリ秒でも秒でも、0 (= 消えない) には落とさない。
        assert_eq!(spec.timeout_millis(), Some(1));
        assert_eq!(spec.timeout_secs(), 1);
    }

    #[test]
    fn positions_split_into_edge_and_alignment() {
        let parts: Vec<_> = ToastPosition::ALL
            .iter()
            .map(|p| (p.is_top(), p.align()))
            .collect();
        assert_eq!(
            parts,
            [
                (true, Align::Start),
                (true, Align::Center),
                (true, Align::End),
                (false, Align::Start),
                (false, Align::Center),
                (false, Align::End),
            ]
        );
    }

    // ---------------------------------------------------------------- 待ち行列

    /// A toast for the tests: records what the queue asked of it in `LOG`.
    struct Fake {
        name: &'static str,
        /// Whether there is somewhere to put it.
        placeable: bool,
        /// Whether its window has closed.
        stranded: std::cell::Cell<bool>,
    }

    thread_local! {
        static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        static QUEUE: RefCell<ToastQueue<Fake>> = const { RefCell::new(ToastQueue::new()) };
        /// Run from inside `present`, to check the queue is not borrowed then.
        static ON_PRESENT: RefCell<Option<Box<dyn FnMut(&Rc<Fake>)>>> =
            const { RefCell::new(None) };
    }

    impl ToastSlot for Fake {
        fn present(this: &Rc<Self>) -> bool {
            LOG.with(|l| l.borrow_mut().push(format!("+{}", this.name)));
            let hook = ON_PRESENT.with(|h| h.borrow_mut().take());
            if let Some(mut hook) = hook {
                hook(this);
            }
            this.placeable
        }

        fn withdraw(this: &Rc<Self>) {
            LOG.with(|l| l.borrow_mut().push(format!("-{}", this.name)));
        }

        fn is_stranded(this: &Rc<Self>) -> bool {
            this.stranded.get()
        }
    }

    fn fake(name: &'static str) -> Rc<Fake> {
        Rc::new(Fake {
            name,
            placeable: true,
            stranded: Default::default(),
        })
    }

    /// Empties the queue and the log, so each test starts afresh.
    fn reset() {
        QUEUE.with(|q| *q.borrow_mut() = ToastQueue::new());
        LOG.with(|l| l.borrow_mut().clear());
    }

    fn log() -> Vec<String> {
        LOG.with(|l| std::mem::take(&mut *l.borrow_mut()))
    }

    fn show(t: &Rc<Fake>, priority: ToastPriority) {
        ToastQueue::show(&QUEUE, t, priority);
    }

    fn remove(t: &Rc<Fake>) {
        ToastQueue::remove(&QUEUE, t);
    }

    fn shown(t: &Rc<Fake>) -> bool {
        ToastQueue::is_shown_in(&QUEUE, t)
    }

    fn waiting(t: &Rc<Fake>) -> bool {
        ToastQueue::is_waiting_in(&QUEUE, t)
    }

    #[test]
    fn normal_toasts_wait_their_turn() {
        reset();
        let (a, b, c) = (fake("a"), fake("b"), fake("c"));
        show(&a, ToastPriority::Normal);
        show(&b, ToastPriority::Normal);
        show(&c, ToastPriority::Normal);
        assert_eq!(log(), ["+a"], "後のものは並ぶだけで出ない");
        assert!(shown(&a) && waiting(&b) && waiting(&c));

        remove(&a);
        assert_eq!(log(), ["-a", "+b"], "前のものが消えると次が出る");
        remove(&b);
        assert_eq!(log(), ["-b", "+c"]);
        remove(&c);
        assert_eq!(log(), ["-c"]);
        assert!(!shown(&c) && !waiting(&c));
    }

    #[test]
    fn high_toasts_come_up_at_once_and_put_the_current_one_back_in_front() {
        reset();
        let (a, b, urgent) = (fake("a"), fake("b"), fake("urgent"));
        show(&a, ToastPriority::Normal);
        show(&b, ToastPriority::Normal);
        log();

        show(&urgent, ToastPriority::High);
        assert_eq!(log(), ["-a", "+urgent"]);
        assert!(shown(&urgent) && waiting(&a));

        remove(&urgent);
        assert_eq!(log(), ["-urgent", "+a"], "割り込まれたものが先に戻る");
        remove(&a);
        assert_eq!(log(), ["-a", "+b"]);
    }

    #[test]
    fn showing_the_current_toast_again_puts_it_up_afresh() {
        reset();
        let (a, b) = (fake("a"), fake("b"));
        show(&a, ToastPriority::Normal);
        show(&b, ToastPriority::Normal);
        log();
        show(&a, ToastPriority::Normal);
        assert_eq!(log(), ["-a", "+a"], "出し直して時間を数え直す");
        assert!(shown(&a) && waiting(&b), "並び順は変わらない");
    }

    #[test]
    fn showing_a_waiting_toast_again_keeps_its_place_unless_it_is_high() {
        reset();
        let (a, b, c) = (fake("a"), fake("b"), fake("c"));
        show(&a, ToastPriority::Normal);
        show(&b, ToastPriority::Normal);
        show(&c, ToastPriority::Normal);
        show(&b, ToastPriority::Normal);
        assert_eq!(log(), ["+a"]);
        assert_eq!(
            QUEUE.with(|q| q.borrow().waiting_len()),
            2,
            "二重に並ばない"
        );

        show(&c, ToastPriority::High);
        assert_eq!(log(), ["-a", "+c"]);
        assert_eq!(QUEUE.with(|q| q.borrow().waiting_len()), 2, "a と b");
        remove(&c);
        remove(&a);
        assert_eq!(log(), ["-c", "+a", "-a", "+b"]);
    }

    #[test]
    fn removing_a_waiting_toast_only_takes_it_out_of_the_queue() {
        reset();
        let (a, b, c) = (fake("a"), fake("b"), fake("c"));
        show(&a, ToastPriority::Normal);
        show(&b, ToastPriority::Normal);
        show(&c, ToastPriority::Normal);
        log();
        remove(&b);
        assert!(log().is_empty(), "画面には何も起きない");
        remove(&a);
        assert_eq!(log(), ["-a", "+c"], "外したものは飛ばされる");

        remove(&a);
        assert!(log().is_empty(), "もう居ないものを外しても何も起きない");
    }

    #[test]
    fn toasts_with_nowhere_to_go_are_dropped_and_the_next_comes_up() {
        reset();
        let a = fake("a");
        let lost = Rc::new(Fake {
            name: "lost",
            placeable: false,
            stranded: Default::default(),
        });
        let c = fake("c");
        show(&a, ToastPriority::Normal);
        show(&lost, ToastPriority::Normal);
        show(&c, ToastPriority::Normal);
        log();

        remove(&a);
        assert_eq!(
            log(),
            ["-a", "+lost", "+c"],
            "出せなかったものは引っ込めずに捨て、次へ進む"
        );
        assert!(!shown(&lost) && !waiting(&lost));
        assert!(shown(&c));

        // 何も出ていないときに出せなかったものは、並びにも残らない。
        remove(&c);
        log();
        show(&lost, ToastPriority::Normal);
        assert_eq!(log(), ["+lost"]);
        assert!(!shown(&lost) && !waiting(&lost));
    }

    #[test]
    fn the_queue_is_free_while_a_toast_comes_up() {
        reset();
        let (a, b, c) = (fake("a"), fake("b"), fake("c"));
        show(&a, ToastPriority::Normal);
        show(&b, ToastPriority::Normal);
        log();
        // b が出る途中で、別のものを並べ、自分が出ているかを確かめる。
        let c_inner = c.clone();
        ON_PRESENT.with(|h| {
            *h.borrow_mut() = Some(Box::new(move |this: &Rc<Fake>| {
                assert!(ToastQueue::is_shown_in(&QUEUE, this));
                ToastQueue::show(&QUEUE, &c_inner, ToastPriority::Normal);
            }))
        });
        remove(&a);
        assert_eq!(log(), ["-a", "+b"]);
        assert!(waiting(&c));
    }

    #[test]
    fn a_toast_left_in_a_closed_window_gives_way() {
        reset();
        let (a, b, c) = (fake("a"), fake("b"), fake("c"));
        show(&a, ToastPriority::Normal);
        show(&b, ToastPriority::Normal);
        log();
        a.stranded.set(true);
        show(&c, ToastPriority::Normal);
        assert_eq!(log(), ["-a", "+b"], "取り残されたものを外し、次を出す");
        assert!(!shown(&a) && shown(&b) && waiting(&c));
    }
}
