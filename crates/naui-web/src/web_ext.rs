//! Web-only DOM operations (CSS classes and attributes).
//!
//! The way for apps to add **things that only mean something on the Web**, such as
//! custom styling or markers for tests (`data-testid` and the like). Other platforms
//! have no counterpart, so it is not in naui's common API but in this separate
//! trait. It works with any widget.
//!
//! ```ignore
//! #[cfg(target_arch = "wasm32")]
//! {
//!     use naui::web::WebWidgetExt;
//!     button.add_class("primary");
//!     button.set_attribute("data-testid", "save");
//! }
//! ```

use crate::widgets::Widget;

/// Attributes naui writes, which the app cannot change.
///
/// - `style`: size specifications (`set_sizing`) and the like are written as inline
///   styles. Change the look with classes and the page's CSS.
/// - `data-naui-*`: used by naui for the parent container's kind and hidden markers.
fn is_reserved(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == "style" || name.starts_with("data-naui-")
}

/// Handles the CSS classes and attributes of a widget's element. Web only.
///
/// The target is the element placed into the parent ([`Widget::native_element`]).
/// naui uses no classes, so you can add and remove classes freely. Among attributes,
/// naui also writes `role`, `aria-*`, `tabindex` and others, so overwriting them can
/// change the widget's behavior or how it is read aloud. `title` is the same
/// attribute that `set_tooltip` writes, so whichever is written last wins.
pub trait WebWidgetExt: Widget {
    /// Adds a CSS class.
    fn add_class(&self, name: &str) {
        let _ = self.native_element().class_list().add_1(name);
    }

    /// Removes a CSS class.
    fn remove_class(&self, name: &str) {
        let _ = self.native_element().class_list().remove_1(name);
    }

    /// Whether a CSS class is present.
    fn has_class(&self, name: &str) -> bool {
        self.native_element().class_list().contains(name)
    }

    /// Writes an attribute. `true` if it was written.
    ///
    /// naui uses `style` and `data-naui-*`, so they are not written (returns `false`).
    fn set_attribute(&self, name: &str, value: &str) -> bool {
        if is_reserved(name) {
            return false;
        }
        self.native_element().set_attribute(name, value).is_ok()
    }

    /// Removes an attribute. `true` if it was removed (including when it was not there
    /// to begin with).
    ///
    /// naui uses `style` and `data-naui-*`, so they are not removed (returns `false`).
    fn remove_attribute(&self, name: &str) -> bool {
        if is_reserved(name) {
            return false;
        }
        self.native_element().remove_attribute(name).is_ok()
    }

    /// The attribute's value. `None` if absent.
    fn attribute(&self, name: &str) -> Option<String> {
        self.native_element().get_attribute(name)
    }
}

impl<T: Widget + ?Sized> WebWidgetExt for T {}
