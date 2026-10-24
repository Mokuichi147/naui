//! Reading and writing text on the clipboard.
//!
//! Reading is asynchronous on some platforms (GTK4, WinRT, browsers), and writing
//! is asynchronous in browsers too, so on all four platforms the result of reading
//! and writing is returned through a callback. The callback is called on the UI
//! thread after `read_text` / `set_text` has returned (it is not called on the spot
//! even on platforms where it could be synchronous).

use naui_core::{Result, Tasks};
use windows::ApplicationModel::DataTransfer::{
    Clipboard as WinClipboard, DataPackage, StandardDataFormats,
};
use windows_core::HSTRING;

use crate::to_error;

/// The clipboard (`Windows.ApplicationModel.DataTransfer.Clipboard`). Get it with
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
    ///
    /// It is `Flush`ed right after writing so that it remains even after the app
    /// closes.
    pub fn set_text(&self, text: &str, f: impl FnOnce(Result<()>) + 'static) {
        let result = write(text);
        self.tasks.spawn(async move { f(result) });
    }

    /// Reads text. Passes `None` if the clipboard holds no text.
    pub fn read_text(&self, f: impl FnOnce(Option<String>) + 'static) {
        self.tasks.spawn(async move {
            f(read().await);
        });
    }
}

fn write(text: &str) -> Result<()> {
    let package = DataPackage::new().map_err(|e| to_error("クリップボードの中身の生成", e))?;
    package
        .SetText(&HSTRING::from(text))
        .map_err(|e| to_error("クリップボードの中身の設定", e))?;
    WinClipboard::SetContent(&package).map_err(|e| to_error("クリップボードへの書き込み", e))?;
    let _ = WinClipboard::Flush();
    Ok(())
}

async fn read() -> Option<String> {
    let content = WinClipboard::GetContent().ok()?;
    let format = StandardDataFormats::Text().ok()?;
    if !content.Contains(&format).unwrap_or(false) {
        return None;
    }
    let text = content.GetTextAsync().ok()?.await.ok()?;
    Some(text.to_string())
}
