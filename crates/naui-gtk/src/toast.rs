//! Toasts (`AdwToast`).
//!
//! **This is the only one of the four platforms with a native toast.** naui's shape
//! for toasts (text, action buttons, time until it disappears) maps onto
//! `AdwToast`. Where it appears (bottom center), how they animate in and out, and
//! screen reader announcements are all done by `AdwToastOverlay`.
//!
//! Changing an `AdwToast`'s contents after adding it would be fine, but it is
//! **recreated each time it comes up** (the same as `Dialog` rebuilding its
//! `AdwAlertDialog`). `Toast` itself holds only "what to show".
//!
//! ## The queue
//!
//! `AdwToastOverlay` queues toasts itself, but one queue per window, and it has no
//! signal for "a waiting toast came up", so `is_visible()` could not be answered
//! from it. The order is therefore kept by [`naui_core::ToastQueue`], shared with
//! the other backends (modelled on `AdwToastOverlay`'s order: `High` comes up at
//! once and puts the current one back at the front; unlike the overlay, showing a
//! waiting toast again does not move it forward), and only the toast whose turn it
//! is is added to the overlay.
//!
//! ## Differences from other platforms
//!
//! - `adw_toast_set_timeout` accepts only **seconds**, so anything under one second
//!   becomes one second ([`ToastSpec::timeout_secs`]).
//! - `AdwToast` has **one** button. With two or more actions, they are all laid out
//!   after the text in a custom title (`adw_toast_set_custom_title`) instead, in the
//!   same order as elsewhere.
//! - `AdwToastOverlay` has no way to change where toasts appear, so
//!   [`ToastPosition`] is ignored: they are always at the bottom center.
//! - `AdwToast` always has a close button (×). Closing it with that counts as
//!   disappearing by itself, so `on_dismiss` is called.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use naui_core::{ToastPosition, ToastPriority, ToastQueue, ToastSlot, ToastSpec};

use crate::callback::Notifier;

/// The spacing between the text and the buttons in a custom title.
const TITLE_SPACING: i32 = 12;

thread_local! {
    /// The toast on screen and the ones waiting their turn.
    static QUEUE: RefCell<ToastQueue<ToastInner>> = const { RefCell::new(ToastQueue::new()) };
}

/// What is held only while it is on screen.
struct Presentation {
    native: adw::Toast,
    /// The overlay it was added to. Used to tell whether its window has closed.
    overlay: adw::ToastOverlay,
    /// The text in the custom title. Present only with two or more actions.
    title: Option<gtk::Label>,
}

struct ToastInner {
    app: adw::Application,
    spec: RefCell<ToastSpec>,
    /// The real toast, held only while it is on screen.
    shown: RefCell<Option<Presentation>>,
    /// The action button that was clicked, until the toast's `dismissed` arrives.
    clicked: Cell<Option<usize>>,
    on_action: Notifier<usize>,
    on_dismiss: Notifier<()>,
}

impl ToastSlot for ToastInner {
    fn present(this: &Rc<Self>) -> bool {
        let Some(overlay) = Toast(this.clone()).overlay() else {
            return false;
        };
        let spec = this.spec.borrow().clone();
        let native = adw::Toast::new(spec.message());
        native.set_timeout(spec.timeout_secs());
        let toast = Toast(this.clone());
        let title = toast.apply_actions(&native, spec.message(), spec.actions());

        {
            let weak = Rc::downgrade(this);
            native.connect_button_clicked(move |_| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                // 本来のボタンを使うのは操作が 1 つのとき。消えるのは次の idle なので、
                // そこで知らせる。
                inner.clicked.set(Some(0));
            });
        }
        {
            let weak = Rc::downgrade(this);
            native.connect_dismissed(move |_| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                Toast(inner).finish();
            });
        }

        this.clicked.set(None);
        *this.shown.borrow_mut() = Some(Presentation {
            native: native.clone(),
            overlay: overlay.clone(),
            title,
        });
        overlay.add_toast(native);
        true
    }

    fn withdraw(this: &Rc<Self>) {
        // 先に手放しておけば、`dismiss` で飛ぶ `dismissed` は `finish` で素通りする。
        let Some(shown) = this.shown.borrow_mut().take() else {
            return;
        };
        shown.native.dismiss();
    }

    fn is_stranded(this: &Rc<Self>) -> bool {
        let shown = this.shown.borrow();
        let Some(shown) = shown.as_ref() else {
            return false;
        };
        let Some(window) = shown.overlay.root().and_downcast::<gtk::Window>() else {
            return true;
        };
        // 表示する前の窓にも出せるので、見えていないだけでは決めない。
        // 閉じた窓は見えておらず、次に出す先でもなくなっている。
        !window.is_visible()
            && Toast(this.clone())
                .overlay()
                .is_none_or(|overlay| overlay != shown.overlay)
    }
}

/// A transient notification (`AdwToast`).
///
/// It is not a widget, so it does not go into containers (the same as `Dialog`).
#[derive(Clone)]
pub struct Toast(Rc<ToastInner>);

impl Toast {
    pub(crate) fn new(app: &adw::Application, message: &str) -> Self {
        Self(Rc::new(ToastInner {
            app: app.clone(),
            spec: RefCell::new(ToastSpec::new(message)),
            shown: RefCell::new(None),
            clicked: Cell::new(None),
            on_action: Notifier::default(),
            on_dismiss: Notifier::default(),
        }))
    }

    /// The string to show. Calling it while shown updates it on the spot.
    pub fn set_message(&self, message: &str) {
        self.0.spec.borrow_mut().set_message(message);
        if let Some(shown) = self.0.shown.borrow().as_ref() {
            match &shown.title {
                Some(label) => label.set_label(message),
                None => shown.native.set_title(message),
            }
        }
    }

    pub fn message(&self) -> String {
        self.0.spec.borrow().message().to_string()
    }

    /// The action buttons' labels, laid out in this order. **Passing an empty list
    /// removes the buttons.**
    ///
    /// Calling it while shown replaces them on the spot.
    pub fn set_actions<S: AsRef<str>>(&self, labels: &[S]) {
        self.0.spec.borrow_mut().set_actions(labels);
        let spec = self.0.spec.borrow().clone();
        let mut shown = self.0.shown.borrow_mut();
        if let Some(shown) = shown.as_mut() {
            shown.title = self.apply_actions(&shown.native, spec.message(), spec.actions());
        }
    }

    /// The action buttons' labels. Empty if there are none.
    pub fn actions(&self) -> Vec<String> {
        self.0.spec.borrow().actions().to_vec()
    }

    /// Seconds until it disappears automatically. **Passing 0 means it never
    /// disappears automatically.**
    ///
    /// Takes effect from the next time it comes up.
    pub fn set_timeout(&self, seconds: f64) {
        self.0.spec.borrow_mut().set_timeout(seconds);
    }

    pub fn timeout(&self) -> f64 {
        self.0.spec.borrow().timeout()
    }

    /// How it takes its turn when shown. Takes effect from the next
    /// [`show`](Self::show).
    pub fn set_priority(&self, priority: ToastPriority) {
        self.0.spec.borrow_mut().set_priority(priority);
    }

    pub fn priority(&self) -> ToastPriority {
        self.0.spec.borrow().priority()
    }

    /// Where in the window it appears. **Ignored on GTK** (always the bottom
    /// center), but kept so that [`spec`](Self::spec) returns it.
    pub fn set_position(&self, position: ToastPosition) {
        self.0.spec.borrow_mut().set_position(position);
    }

    pub fn position(&self) -> ToastPosition {
        self.0.spec.borrow().position()
    }

    /// The current settings.
    pub fn spec(&self) -> ToastSpec {
        self.0.spec.borrow().clone()
    }

    /// Called with the index of the clicked action button. Setting it again removes
    /// the previous one.
    ///
    /// Clicking it makes the toast disappear, so `on_dismiss` is called next.
    pub fn on_action(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_action.set(f);
    }

    /// Called when it disappears. Setting it again removes the previous one.
    ///
    /// It is called **when it times out**, **when it is closed by an action button**
    /// and when it is closed by `AdwToast`'s close button. It is not called when it is
    /// closed with [`dismiss`](Self::dismiss) or put back in the queue by a `High`
    /// toast (the same rule as [`Dialog::close`](crate::Dialog::close): the app's own
    /// actions are not notified).
    pub fn on_dismiss(&self, mut f: impl FnMut() + 'static) {
        self.0.on_dismiss.set(move |()| f());
    }

    /// Shows the toast: on screen at once, or at the back of the queue (see
    /// [`naui_core::ToastQueue`]). Dropped if there is no window to show it in when
    /// its turn comes.
    pub fn show(&self) {
        let priority = self.0.spec.borrow().priority();
        ToastQueue::show(&QUEUE, &self.0, priority);
    }

    /// Closes the toast, or takes it out of the queue. `on_dismiss` is not called.
    pub fn dismiss(&self) {
        ToastQueue::remove(&QUEUE, &self.0);
    }

    /// Whether it is on screen now.
    pub fn is_visible(&self) -> bool {
        self.0.shown.borrow().is_some()
    }

    /// Whether it is waiting its turn in the queue.
    pub fn is_queued(&self) -> bool {
        ToastQueue::is_waiting_in(&QUEUE, &self.0)
    }

    /// The `AdwToast` on screen now. `None` if it is not on screen.
    ///
    /// Public as a backend-specific escape hatch.
    pub fn native_toast(&self) -> Option<adw::Toast> {
        self.0
            .shown
            .borrow()
            .as_ref()
            .map(|shown| shown.native.clone())
    }

    /// Puts the action buttons on `native`. Returns the label of the custom title, if
    /// one was needed (two or more actions).
    ///
    /// One action is `AdwToast`'s own button. `AdwToast` has room for only one, and
    /// gives the spare width to the space before it, so two or more all go after the
    /// text in a custom title instead, next to each other.
    fn apply_actions(
        &self,
        native: &adw::Toast,
        message: &str,
        actions: &[String],
    ) -> Option<gtk::Label> {
        if actions.len() < 2 {
            native.set_button_label(actions.first().map(String::as_str));
            native.set_title(message);
            return None;
        }
        native.set_button_label(None);

        let row = gtk::Box::new(gtk::Orientation::Horizontal, TITLE_SPACING);
        // AdwToast が自分で作る題と同じ見た目にする (太字・1 行・末尾を省略)。
        let label = gtk::Label::new(Some(message));
        label.add_css_class("heading");
        label.set_xalign(0.0);
        label.set_single_line_mode(true);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_hexpand(true);
        row.append(&label);
        for (index, text) in actions.iter().enumerate() {
            let button = gtk::Button::with_label(text);
            button.set_valign(gtk::Align::Center);
            let weak = Rc::downgrade(&self.0);
            button.connect_clicked(move |_| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                inner.clicked.set(Some(index));
                // 本来のボタンと同じく、押したら消える。`dismissed` から知らせる。
                let native = inner
                    .shown
                    .borrow()
                    .as_ref()
                    .map(|shown| shown.native.clone());
                if let Some(native) = native {
                    native.dismiss();
                }
            });
            row.append(&button);
        }
        native.set_custom_title(Some(&row));
        Some(label)
    }

    /// Cleanup and notification when the `AdwToast` disappears by itself (time ran
    /// out, or a button was clicked).
    ///
    /// When naui took it down itself, `shown` is already empty and nothing happens.
    fn finish(&self) {
        // 先に手放す。並びから外したときの `withdraw` が、`dismissed` の最中に
        // もう一度 `dismiss` を呼んでシグナルを入れ子にしないように。
        if self.0.shown.borrow_mut().take().is_none() {
            return;
        }
        // 次のものを出すのは、`AdwToastOverlay` 自身の `dismissed` の処理より前になる。
        // そのときは overlay がそれを自分の待ち行列へ入れ、今のものを下げた直後に出す。
        ToastQueue::remove(&QUEUE, &self.0);
        if let Some(index) = self.0.clicked.take() {
            self.0.on_action.emit(index);
        }
        self.0.on_dismiss.emit(());
    }

    /// The `AdwToastOverlay` the toast is placed on. `None` if there is no window yet.
    ///
    /// It is shown in the frontmost window. When no window has focus yet (right after
    /// startup, for example), the first window the app holds is used (the same idea as
    /// how `Dialog` picks its parent).
    fn overlay(&self) -> Option<adw::ToastOverlay> {
        let window = self
            .0
            .app
            .active_window()
            .or_else(|| self.0.app.windows().into_iter().next())?;
        crate::window::toast_overlay(&window)
    }
}
