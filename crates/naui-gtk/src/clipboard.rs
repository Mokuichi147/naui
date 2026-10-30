//! Reading and writing text on the clipboard.
//!
//! Reading is asynchronous on some platforms (GTK4, WinRT, browsers), and writing
//! is asynchronous in browsers too, so on all four platforms the result of reading
//! and writing is returned through a callback. The callback is called on the UI
//! thread after `read_text` / `set_text` has returned (it is not called on the spot
//! even on platforms where it could be synchronous).

use gtk::prelude::*;
use gtk::{gdk, gio};
use naui_core::{Error, Result};

/// The clipboard (`GdkClipboard`). Get it with [`Ui::clipboard`](crate::Ui::clipboard).
#[derive(Clone)]
pub struct Clipboard;

impl Clipboard {
    fn native() -> Option<gdk::Clipboard> {
        gdk::Display::default().map(|display| display.clipboard())
    }

    /// Writes text, replacing the previous contents. Whether it was written is passed
    /// to `f`.
    pub fn set_text(&self, text: &str, f: impl FnOnce(Result<()>) + 'static) {
        let result = match Self::native() {
            Some(clipboard) => {
                clipboard.set_text(text);
                Ok(())
            }
            None => Err(Error::new("write to the clipboard", "there is no display")),
        };
        // その場では呼ばない約束なので、ループを 1 周させてから渡す。
        gtk::glib::idle_add_local_once(move || f(result));
    }

    /// Reads text. Passes `None` if the clipboard holds no text.
    pub fn read_text(&self, f: impl FnOnce(Option<String>) + 'static) {
        let Some(clipboard) = Self::native() else {
            // その場では呼ばない約束なので、ループを 1 周させてから渡す。
            gtk::glib::idle_add_local_once(move || f(None));
            return;
        };
        clipboard.read_text_async(None::<&gio::Cancellable>, move |result| {
            f(result.ok().flatten().map(|text| text.to_string()));
        });
    }
}
