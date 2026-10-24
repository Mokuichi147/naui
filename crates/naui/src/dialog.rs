use crate::sys;
use crate::Widget;
use naui_core::{DialogButtons, DialogResponse};

/// A modal dialog.
///
/// It is not a widget, so it does not go into containers (the same as `Window`).
#[derive(Clone)]
pub struct Dialog(pub(crate) sys::Dialog);

impl_backend!(Dialog);

impl Dialog {
    #[inline]
    pub fn set_title(&self, title: &str) {
        self.0.set_title(title)
    }

    #[inline]
    pub fn title(&self) -> String {
        self.0.title()
    }

    #[inline]
    pub fn set_message(&self, message: &str) {
        self.0.set_message(message)
    }

    #[inline]
    pub fn message(&self) -> String {
        self.0.message()
    }

    /// The widget placed below the body text.
    #[inline]
    pub fn set_child(&self, child: &dyn Widget) {
        self.0.set_child(child.as_backend())
    }

    /// The buttons to show. With none specified, only "OK" is shown.
    #[inline]
    pub fn set_buttons(&self, buttons: DialogButtons) {
        self.0.set_buttons(buttons)
    }

    #[inline]
    pub fn buttons(&self) -> DialogButtons {
        self.0.buttons()
    }

    /// Called with the reason when the dialog closes.
    #[inline]
    pub fn on_response(&self, f: impl FnMut(DialogResponse) + 'static) {
        self.0.on_response(f)
    }

    /// Opens it. Does nothing if it is already open.
    #[inline]
    pub fn open(&self) {
        self.0.open()
    }

    /// Closes it. Does nothing if it is not open.
    #[inline]
    pub fn close(&self) {
        self.0.close()
    }

    #[inline]
    pub fn is_open(&self) -> bool {
        self.0.is_open()
    }
}
