//! Dialogs with arbitrary content (WinUI 3's `ContentDialog`).
//!
//! WinUI 3's dialog for arbitrary content is the same **`ContentDialog`** that
//! `Dialog` uses: its `Content` takes any element, and its button row takes up to
//! three buttons with roles. So a sheet is a [`Dialog`] whose body text stays empty,
//! with the content widget as the only thing in its content area.
//!
//! ## Differences from other platforms
//!
//! - `ContentDialog` keeps its width between `ContentDialogMinWidth` (320) and
//!   `ContentDialogMaxWidth` (548) from WinUI's resources, so wide content is
//!   squeezed to that width.
//! - It cannot be shown until a window has been shown, and two dialogs cannot be
//!   shown on the same `XamlRoot` (the same as `Dialog`).

use naui_core::{DialogButtons, DialogResponse, Result, Theme};
use naui_winui3::Microsoft::UI::Xaml::Controls::ContentDialog;

use crate::dialog::Dialog;
use crate::widgets::Widget;

/// A modal dialog with arbitrary content (`ContentDialog`).
///
/// It is not a widget, so it does not go into containers (the same as `Window`).
#[derive(Clone)]
pub struct Sheet(Dialog);

impl Sheet {
    pub(crate) fn new(title: &str, theme: Theme) -> Result<Self> {
        Dialog::new(title, theme).map(Self)
    }

    /// The heading. An empty string shows none.
    pub fn set_title(&self, title: &str) {
        self.0.set_title(title)
    }

    pub fn title(&self) -> String {
        self.0.title()
    }

    /// The content. Each call replaces it.
    pub fn set_child(&self, child: &dyn Widget) {
        self.0.set_child(child)
    }

    /// The buttons to show. With none specified, only "OK" is shown.
    pub fn set_buttons(&self, buttons: DialogButtons) {
        self.0.set_buttons(buttons)
    }

    pub fn buttons(&self) -> DialogButtons {
        self.0.buttons()
    }

    /// Enables or disables the button for `response` (`IsPrimaryButtonEnabled` /
    /// `IsSecondaryButtonEnabled`). The cancel button cannot be disabled.
    pub fn set_button_enabled(&self, response: DialogResponse, enabled: bool) {
        self.0.set_button_enabled(response, enabled)
    }

    pub fn is_button_enabled(&self, response: DialogResponse) -> bool {
        self.0.is_button_enabled(response)
    }

    /// Called with the reason when the user closes it. Setting it again replaces the
    /// previous closure.
    ///
    /// Not called when it is closed with [`Sheet::close`].
    pub fn on_response(&self, f: impl FnMut(DialogResponse) + 'static) {
        self.0.on_response(f)
    }

    /// Shows it. It returns right away, and closing arrives through `on_response`.
    ///
    /// Does nothing if it is already open, or before a window has been shown.
    pub fn open(&self) {
        self.0.open()
    }

    /// Closes it. `on_response` is not called. Does nothing if it is not open.
    pub fn close(&self) {
        self.0.close()
    }

    /// Whether it is open.
    pub fn is_open(&self) -> bool {
        self.0.is_open()
    }

    /// Sets the sheet's color theme.
    pub(crate) fn set_theme(&self, theme: Theme) {
        self.0.set_theme(theme)
    }

    /// The real WinUI 3 dialog. A backend-specific escape hatch.
    pub fn native_dialog(&self) -> ContentDialog {
        self.0.native_dialog()
    }
}
