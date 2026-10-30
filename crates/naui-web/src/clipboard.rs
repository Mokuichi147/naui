//! Reading and writing text on the clipboard.
//!
//! Reading is asynchronous on some platforms (GTK4, WinRT, browsers), and writing
//! is asynchronous in browsers too, so on all four platforms the result of reading
//! and writing is returned through a callback. The callback is called on the UI
//! thread after `read_text` / `set_text` has returned (it is not called on the spot
//! even on platforms where it could be synchronous).
//!
//! The browser's Clipboard API is available only in secure contexts (https or
//! localhost), and reading may ask the user for permission.

use naui_core::{Error, Result};

/// The clipboard (`navigator.clipboard`). Get it with
/// [`Ui::clipboard`](crate::Ui::clipboard).
#[derive(Clone)]
pub struct Clipboard;

impl Clipboard {
    fn native() -> Option<web_sys::Clipboard> {
        web_sys::window().map(|window| window.navigator().clipboard())
    }

    /// Writes text, replacing the previous contents. Whether it was written is passed
    /// to `f`.
    ///
    /// Browsers may allow writing only inside a user action (a click, for example), and
    /// refuse inside an `<iframe>` without permission. When refused, `Err` arrives, so
    /// call it from a button's `on_click` or similar and report something like "Copied"
    /// only after checking the result.
    pub fn set_text(&self, text: &str, f: impl FnOnce(Result<()>) + 'static) {
        let promise = Self::native().map(|clipboard| clipboard.write_text(text));
        js_sys::futures::spawn_local(async move {
            let result = match promise {
                Some(promise) => js_sys::futures::JsFuture::from(promise)
                    .await
                    .map(|_| ())
                    .map_err(|e| crate::to_error("write to the clipboard", e)),
                None => Err(Error::new(
                    "write to the clipboard",
                    "the Clipboard API is not available",
                )),
            };
            f(result);
        });
    }

    /// Reads text. Passes `None` if the clipboard holds no text or it could not be
    /// read.
    pub fn read_text(&self, f: impl FnOnce(Option<String>) + 'static) {
        let promise = Self::native().map(|clipboard| clipboard.read_text());
        js_sys::futures::spawn_local(async move {
            let text = match promise {
                Some(promise) => js_sys::futures::JsFuture::from(promise)
                    .await
                    .ok()
                    .and_then(|value| value.as_string()),
                None => None,
            };
            f(text.filter(|text| !text.is_empty()));
        });
    }
}
