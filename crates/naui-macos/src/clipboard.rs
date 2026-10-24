//! Reading and writing text on the clipboard.
//!
//! Reading is asynchronous on some platforms (GTK4, WinRT, browsers), and writing
//! is asynchronous in browsers too, so on all four platforms the result of reading
//! and writing is returned through a callback. The callback is called on the UI
//! thread after `read_text` / `set_text` has returned (it is not called on the spot
//! even on platforms where it could be synchronous).

use naui_core::{Error, Result, Tasks};
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::NSString;

/// The clipboard (the general `NSPasteboard`). Get it with
/// [`Ui::clipboard`](crate::Ui::clipboard).
#[derive(Clone)]
pub struct Clipboard {
    tasks: Tasks,
}

impl Clipboard {
    pub(crate) fn new(tasks: Tasks) -> Self {
        Self { tasks }
    }

    /// Writes text, replacing the previous contents. Whether it was written is passed
    /// to `f`.
    pub fn set_text(&self, text: &str, f: impl FnOnce(Result<()>) + 'static) {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        let result = if pasteboard
            .setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeString })
        {
            Ok(())
        } else {
            Err(Error::new(
                "クリップボードへの書き込み",
                "NSPasteboard が受け付けませんでした",
            ))
        };
        self.tasks.spawn(async move { f(result) });
    }

    /// Reads text. Passes `None` if the clipboard holds no text.
    pub fn read_text(&self, f: impl FnOnce(Option<String>) + 'static) {
        let text = NSPasteboard::generalPasteboard()
            .stringForType(unsafe { NSPasteboardTypeString })
            .map(|text| text.to_string());
        self.tasks.spawn(async move { f(text) });
    }
}
