use crate::sys;
use naui_core::Result;

/// The clipboard (`GdkClipboard`). Get it with [`Ui::clipboard`](crate::Ui::clipboard).
#[derive(Clone)]
pub struct Clipboard(pub(crate) sys::Clipboard);

impl_backend!(Clipboard);

impl Clipboard {
    /// Writes text, replacing the previous contents. Whether it was written is passed
    /// to `f`.
    #[inline]
    pub fn set_text(&self, text: &str, f: impl FnOnce(Result<()>) + 'static) {
        self.0.set_text(text, f)
    }

    /// Reads text. Passes `None` if the clipboard holds no text.
    #[inline]
    pub fn read_text(&self, f: impl FnOnce(Option<String>) + 'static) {
        self.0.read_text(f)
    }
}
