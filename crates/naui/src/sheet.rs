use crate::sys;
use crate::Widget;
use naui_core::{DialogButtons, DialogResponse};

/// A modal dialog holding any widget as its content, with buttons by role.
///
/// For forms and settings that do not fit [`Dialog`](crate::Dialog)'s message and
/// buttons: creating a folder, editing an entry, choosing options. It is the
/// platform's dialog for arbitrary content (`ContentDialog`, `AdwDialog`, a sheet
/// on macOS, `<dialog>`), and naui places the buttons where the platform puts them.
///
/// It is not a widget, so it does not go into containers (the same as `Window`).
/// See [Sheets](crate::guide#sheets).
#[derive(Clone)]
pub struct Sheet(pub(crate) sys::Sheet);

impl_backend!(Sheet);

impl Sheet {
    /// The heading. An empty string shows none.
    #[inline]
    pub fn set_title(&self, title: &str) {
        self.0.set_title(title)
    }

    #[inline]
    pub fn title(&self) -> String {
        self.0.title()
    }

    /// The content. Each call replaces it.
    ///
    /// The sheet takes the content's natural size, so `Fill` in the content's
    /// [`set_sizing`](crate::Label::set_sizing) has nothing to fill. Give a fixed size
    /// or a minimum to make it larger.
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

    /// Enables or disables the button for `response`, for example to keep "Create"
    /// disabled until a name is entered. Buttons are enabled by default, and the
    /// setting is kept when the buttons are replaced.
    ///
    /// The cancel button cannot be disabled (Esc closes the sheet anyway), so
    /// [`DialogResponse::Cancel`] is ignored.
    #[inline]
    pub fn set_button_enabled(&self, response: DialogResponse, enabled: bool) {
        self.0.set_button_enabled(response, enabled)
    }

    #[inline]
    pub fn is_button_enabled(&self, response: DialogResponse) -> bool {
        self.0.is_button_enabled(response)
    }

    /// Called with the reason when the user closes it: the role of the clicked
    /// button, or [`DialogResponse::Cancel`] for Esc.
    ///
    /// Not called when it is closed with [`close`](Self::close). Setting it again
    /// replaces the previous closure.
    #[inline]
    pub fn on_response(&self, f: impl FnMut(DialogResponse) + 'static) {
        self.0.on_response(f)
    }

    /// Shows it over the window. It returns right away on every platform, and
    /// closing arrives through [`on_response`](Self::on_response).
    ///
    /// Does nothing if it is already open, or while no window is shown.
    #[inline]
    pub fn open(&self) {
        self.0.open()
    }

    /// Closes it. [`on_response`](Self::on_response) is not called. Does nothing if
    /// it is not open.
    #[inline]
    pub fn close(&self) {
        self.0.close()
    }

    #[inline]
    pub fn is_open(&self) -> bool {
        self.0.is_open()
    }
}
