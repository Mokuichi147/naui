use crate::sys;
use naui_core::{ToastPosition, ToastPriority, ToastSpec};

/// A transient notification (`AdwToast`).
///
/// It is not a widget, so it does not go into containers (the same as `Dialog`).
///
/// **One toast is on screen at a time; the others wait their turn** (modelled on
/// `AdwToastOverlay`). [`show`](Self::show) puts it at the back of the queue, or
/// on screen at once with [`ToastPriority::High`].
#[derive(Clone)]
pub struct Toast(pub(crate) sys::Toast);

impl_backend!(Toast);

impl Toast {
    /// The string to show. Calling it while shown updates it on the spot.
    #[inline]
    pub fn set_message(&self, message: &str) {
        self.0.set_message(message)
    }

    #[inline]
    pub fn message(&self) -> String {
        self.0.message()
    }

    /// The action buttons' labels, laid out in this order from the start side.
    /// **Passing an empty list removes the buttons.** Calling it while shown updates
    /// them on the spot.
    ///
    /// [`on_action`](Self::on_action) receives the index of the clicked one in this
    /// list. Toasts are meant for one or two short actions; more than that belongs in
    /// a [`Dialog`](crate::Dialog) or a [`Sheet`](crate::Sheet).
    ///
    /// `AdwToast` has a single button, so on GTK two or more are laid out after the
    /// text in its custom title instead.
    #[inline]
    pub fn set_actions<S: AsRef<str>>(&self, labels: &[S]) {
        self.0.set_actions(labels)
    }

    /// The action buttons' labels. Empty if there are none.
    #[inline]
    pub fn actions(&self) -> Vec<String> {
        self.0.actions()
    }

    /// Seconds until it disappears automatically, counted from when it comes up on
    /// screen. **Passing 0 means it never disappears automatically.**
    ///
    /// Takes effect from the next time it comes up.
    #[inline]
    pub fn set_timeout(&self, seconds: f64) {
        self.0.set_timeout(seconds)
    }

    #[inline]
    pub fn timeout(&self) -> f64 {
        self.0.timeout()
    }

    /// How it takes its turn when shown: waiting behind the others
    /// ([`Normal`](ToastPriority::Normal), the default) or coming up at once
    /// ([`High`](ToastPriority::High)).
    ///
    /// Takes effect from the next [`show`](Self::show).
    #[inline]
    pub fn set_priority(&self, priority: ToastPriority) {
        self.0.set_priority(priority)
    }

    #[inline]
    pub fn priority(&self) -> ToastPriority {
        self.0.priority()
    }

    /// Where in the window it appears. The default is the bottom center.
    ///
    /// Takes effect from the next time it comes up. **GTK ignores it**:
    /// `AdwToastOverlay` always places toasts at the bottom center.
    #[inline]
    pub fn set_position(&self, position: ToastPosition) {
        self.0.set_position(position)
    }

    #[inline]
    pub fn position(&self) -> ToastPosition {
        self.0.position()
    }

    /// The current settings.
    #[inline]
    pub fn spec(&self) -> ToastSpec {
        self.0.spec()
    }

    /// Called with the index (in [`actions`](Self::actions)) of the clicked action
    /// button. Setting it again removes the previous one.
    ///
    /// Clicking it makes the toast disappear, so `on_dismiss` is called next.
    #[inline]
    pub fn on_action(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_action(f)
    }

    /// Called when it disappears. Setting it again removes the previous one.
    ///
    /// It is called **when it times out** and **when it is closed by an action
    /// button** (or, on GTK, by `AdwToast`'s close button). It is not called when it
    /// is closed with [`dismiss`](Self::dismiss), nor when a
    /// [`High`](ToastPriority::High) toast puts it back in the queue (the same rule
    /// as [`Dialog::close`](crate::Dialog::close): the app's own actions are not
    /// notified). The next toast in the queue is already on screen when it is called.
    #[inline]
    pub fn on_dismiss(&self, f: impl FnMut() + 'static) {
        self.0.on_dismiss(f)
    }

    /// Shows the toast.
    ///
    /// With nothing on screen it comes up at once, in the frontmost window. Otherwise
    /// it waits at the back of the queue, or, with [`ToastPriority::High`], comes up
    /// at once and puts the one on screen back at the front of the queue. Its time
    /// starts when it comes up.
    ///
    /// Showing it again while it is on screen puts it up afresh (the time starts
    /// over); while it waits, it keeps its place. If there is no window to show it in
    /// when its turn comes, it is dropped. A toast left on screen in a window that
    /// has since been closed is taken down first, without `on_dismiss`.
    ///
    /// To replace the toast on screen instead, [`dismiss`](Self::dismiss) it first.
    #[inline]
    pub fn show(&self) {
        self.0.show()
    }

    /// Closes the toast, or takes it out of the queue if it is waiting. `on_dismiss`
    /// is not called.
    #[inline]
    pub fn dismiss(&self) {
        self.0.dismiss()
    }

    /// Whether it is on screen now.
    #[inline]
    pub fn is_visible(&self) -> bool {
        self.0.is_visible()
    }

    /// Whether it is waiting its turn in the queue.
    #[inline]
    pub fn is_queued(&self) -> bool {
        self.0.is_queued()
    }

    /// Clicks its action button at `index` (in [`actions`](Self::actions)) the way
    /// the user does, so [`on_action`](Self::on_action) is called with `index` and
    /// the toast disappears, calling [`on_dismiss`](Self::on_dismiss). Nothing
    /// happens unless it is on screen ([`is_visible`](Self::is_visible)) and has a
    /// button at `index`.
    ///
    /// For tests and automation. See [Testing apps](crate::guide#testing-apps).
    #[inline]
    pub fn click_action(&self, index: usize) {
        self.0.click_action(index)
    }
}
