//! Dialogs with arbitrary content (DOM).
//!
//! The browser's dialog for arbitrary content is the same **`<dialog>`** that
//! `Dialog` uses, shown with `showModal()`. So a sheet is a [`Dialog`] whose body
//! text stays empty: the title as an `<h2>`, the content widget, and the buttons
//! laid out at the bottom right with the primary action at the right end.
//!
//! ## Differences from other platforms
//!
//! The browser draws the `<dialog>` with its own look (a border on `Canvas`), and
//! nothing marks the primary button but the `data-naui-role="primary"` attribute,
//! which an app's CSS can select.

use naui_core::{DialogButtons, DialogResponse, Result, SHEET_MIN_WIDTH};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement};

use crate::dialog::Dialog;
use crate::widgets::Widget;

/// A modal dialog with arbitrary content (`<dialog>`).
///
/// It is not a widget, so it does not go into containers (the same as `Window`).
#[derive(Clone)]
pub struct Sheet(Dialog);

impl Sheet {
    pub(crate) fn new(doc: &Document, title: &str) -> Result<Self> {
        let dialog = Dialog::new(doc, title)?;
        // 狭い画面では、最小の幅より画面を優先する。
        if let Some(element) = dialog.native_element().dyn_ref::<HtmlElement>() {
            let _ = element.style().set_property(
                "min-width",
                &format!("min({SHEET_MIN_WIDTH}px, calc(100vw - 4em))"),
            );
        }
        Ok(Self(dialog))
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

    /// Enables or disables the button for `response` (the `disabled` attribute).
    /// The cancel button cannot be disabled.
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

    /// Shows it. **It returns right away, and closing arrives through
    /// `on_response`.** Does nothing if it is already open.
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

    /// The `<dialog>` element. A backend-specific escape hatch.
    pub fn native_element(&self) -> Element {
        self.0.native_element()
    }
}
