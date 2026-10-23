//! Toasts (`AdwToast`).
//!
//! **This is the only one of the four platforms with a native toast.** naui's shape
//! for toasts (text, action button, time until it disappears) maps directly onto
//! `AdwToast`. Where it appears (bottom center), how they stack, how they animate
//! out, and screen reader announcements are all done by `AdwToastOverlay`.
//!
//! Changing an `AdwToast`'s contents after adding it would be fine, but it is
//! **recreated each time it is shown** (the same as `Dialog` rebuilding its
//! `AdwAlertDialog`). `Toast` itself holds only "what to show".
//!
//! ## Differences from other platforms
//!
//! `adw_toast_set_timeout` accepts only **seconds**, so anything under one second
//! becomes one second ([`ToastSpec::timeout_secs`]).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use naui_core::ToastSpec;

use crate::callback::Notifier;

thread_local! {
    /// The toast showing now. Only one is shown at a time, and a new one replaces it.
    static CURRENT: RefCell<Option<Toast>> = const { RefCell::new(None) };
}

struct ToastInner {
    app: adw::Application,
    spec: RefCell<ToastSpec>,
    /// The real toast, held only while it is showing.
    current: RefCell<Option<adw::Toast>>,
    on_action: Notifier<()>,
    on_dismiss: Notifier<()>,
    /// Whether the app closed it. A marker so that the reason for closing is not
    /// notified.
    dismissed_by_app: Cell<bool>,
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
            current: RefCell::new(None),
            on_action: Notifier::default(),
            on_dismiss: Notifier::default(),
            dismissed_by_app: Cell::new(false),
        }))
    }

    /// The string to show. Calling it while shown updates it on the spot.
    pub fn set_message(&self, message: &str) {
        self.0.spec.borrow_mut().set_message(message);
        if let Some(native) = self.0.current.borrow().as_ref() {
            native.set_title(message);
        }
    }

    pub fn message(&self) -> String {
        self.0.spec.borrow().message().to_string()
    }

    /// The action button's label. **Passing an empty string removes the button.**
    pub fn set_action(&self, label: &str) {
        self.0.spec.borrow_mut().set_action(label);
        if let Some(native) = self.0.current.borrow().as_ref() {
            native.set_button_label(self.0.spec.borrow().action());
        }
    }

    /// The action button's label. The empty string if there is no button.
    pub fn action(&self) -> String {
        self.0
            .spec
            .borrow()
            .action()
            .unwrap_or_default()
            .to_string()
    }

    /// Seconds until it disappears automatically. **Passing 0 means it never
    /// disappears automatically.**
    ///
    /// Takes effect from the next [`show`](Self::show).
    pub fn set_timeout(&self, seconds: f64) {
        self.0.spec.borrow_mut().set_timeout(seconds);
    }

    pub fn timeout(&self) -> f64 {
        self.0.spec.borrow().timeout()
    }

    /// The current settings.
    pub fn spec(&self) -> ToastSpec {
        self.0.spec.borrow().clone()
    }

    /// Called when the action button is clicked. Setting it again removes the previous
    /// one.
    ///
    /// Clicking it makes the toast disappear, so `on_dismiss` is called next.
    pub fn on_action(&self, mut f: impl FnMut() + 'static) {
        self.0.on_action.set(move |()| f());
    }

    /// Called when it disappears. Setting it again removes the previous one.
    ///
    /// It is called **when it times out** and **when it is closed by the action
    /// button**. It is not called when it is closed with [`dismiss`](Self::dismiss) or
    /// replaced by another toast (the same rule as
    /// [`Dialog::close`](crate::Dialog::close): the app's own actions are not
    /// notified).
    pub fn on_dismiss(&self, mut f: impl FnMut() + 'static) {
        self.0.on_dismiss.set(move |()| f());
    }

    /// Shows the toast.
    ///
    /// **Only one is shown at a time**; if another toast is showing, it is replaced
    /// (the replaced one's `on_dismiss` is not called). Does nothing if there is no
    /// window to show it in yet.
    ///
    /// `AdwToastOverlay` is designed to queue added toasts, but to behave the same on
    /// all four platforms, naui removes the previous one before adding.
    pub fn show(&self) {
        let Some(overlay) = self.overlay() else {
            return;
        };
        if let Some(previous) = CURRENT.with(|slot| slot.borrow_mut().take()) {
            previous.take_down();
        }

        let spec = self.0.spec.borrow().clone();
        let native = adw::Toast::new(spec.message());
        native.set_timeout(spec.timeout_secs());
        native.set_button_label(spec.action());

        {
            let weak = Rc::downgrade(&self.0);
            native.connect_button_clicked(move |_| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                inner.on_action.emit(());
            });
        }
        {
            let weak = Rc::downgrade(&self.0);
            native.connect_dismissed(move |_| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                Toast(inner).finish();
            });
        }

        *self.0.current.borrow_mut() = Some(native.clone());
        self.0.dismissed_by_app.set(false);
        CURRENT.with(|slot| *slot.borrow_mut() = Some(self.clone()));
        overlay.add_toast(native);
    }

    /// Closes the toast being shown. `on_dismiss` is not called.
    pub fn dismiss(&self) {
        if !self.is_visible() {
            return;
        }
        self.forget_current();
        self.take_down();
    }

    /// Whether it is showing now.
    pub fn is_visible(&self) -> bool {
        self.0.current.borrow().is_some()
    }

    /// The `AdwToast` showing now. `None` if none is showing.
    ///
    /// Public as a backend-specific escape hatch.
    pub fn native_toast(&self) -> Option<adw::Toast> {
        self.0.current.borrow().clone()
    }

    /// Cleanup and notification when the `AdwToast` disappears.
    fn finish(&self) {
        if self.0.current.borrow_mut().take().is_none() {
            return; // すでに片付けてある。
        }
        self.forget_current();
        if self.0.dismissed_by_app.replace(false) {
            return; // アプリ側から消した。通知はしない。
        }
        self.0.on_dismiss.emit(());
    }

    /// Closes the toast without notifying.
    fn take_down(&self) {
        let native = self.0.current.borrow().clone();
        let Some(native) = native else {
            return;
        };
        // 消せば `dismissed` が飛ぶので、後始末はそちらの経路で済む。
        self.0.dismissed_by_app.set(true);
        native.dismiss();
        // ウィンドウが閉じたあとなど、`dismissed` が来なかったときの受け皿。
        self.0.current.borrow_mut().take();
        self.0.dismissed_by_app.set(false);
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

    /// If "the toast showing now" is this one, vacates that slot.
    fn forget_current(&self) {
        CURRENT.with(|slot| {
            let mine = slot
                .borrow()
                .as_ref()
                .is_some_and(|current| Rc::ptr_eq(&current.0, &self.0));
            if mine {
                slot.borrow_mut().take();
            }
        });
    }
}
