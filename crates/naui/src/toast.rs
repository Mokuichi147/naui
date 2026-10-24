use crate::sys;
use naui_core::ToastSpec;

/// A transient notification (`AdwToast`).
///
/// It is not a widget, so it does not go into containers (the same as `Dialog`).
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

    /// The action button's label. **Passing an empty string removes the button.**
    #[inline]
    pub fn set_action(&self, label: &str) {
        self.0.set_action(label)
    }

    /// The action button's label. The empty string if there is no button.
    #[inline]
    pub fn action(&self) -> String {
        self.0.action()
    }

    /// Seconds until it disappears automatically. **Passing 0 means it never
    /// disappears automatically.**
    ///
    /// Takes effect from the next [`show`](Self::show).
    #[inline]
    pub fn set_timeout(&self, seconds: f64) {
        self.0.set_timeout(seconds)
    }

    #[inline]
    pub fn timeout(&self) -> f64 {
        self.0.timeout()
    }

    /// The current settings.
    #[inline]
    pub fn spec(&self) -> ToastSpec {
        self.0.spec()
    }

    /// Called when the action button is clicked. Setting it again removes the previous
    /// one.
    ///
    /// Clicking it makes the toast disappear, so `on_dismiss` is called next.
    #[inline]
    pub fn on_action(&self, f: impl FnMut() + 'static) {
        self.0.on_action(f)
    }

    /// Called when it disappears. Setting it again removes the previous one.
    ///
    /// It is called **when it times out** and **when it is closed by the action
    /// button**. It is not called when it is closed with [`dismiss`](Self::dismiss) or
    /// replaced by another toast (the same rule as
    /// [`Dialog::close`](crate::Dialog::close): the app's own actions are not
    /// notified).
    #[inline]
    pub fn on_dismiss(&self, f: impl FnMut() + 'static) {
        self.0.on_dismiss(f)
    }

    /// Shows the toast.
    ///
    /// **Only one is shown at a time**; if another toast is showing, it is replaced
    /// (the replaced one's `on_dismiss` is not called). Does nothing if there is no
    /// window to show it in yet.
    ///
    /// `AdwToastOverlay` is designed to queue added toasts, but to behave the same on
    /// all four platforms, naui removes the previous one before adding.
    #[inline]
    pub fn show(&self) {
        self.0.show()
    }

    /// Closes the toast being shown. `on_dismiss` is not called.
    #[inline]
    pub fn dismiss(&self) {
        self.0.dismiss()
    }

    /// Whether it is showing now.
    #[inline]
    pub fn is_visible(&self) -> bool {
        self.0.is_visible()
    }
}
