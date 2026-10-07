//! クリップボードの文字の読み書き。
//!
//! 読むほうは環境によって非同期 (GTK4・WinRT・ブラウザ) なので、4 環境とも
//! 結果をコールバックで返す。コールバックは UI スレッドで、`read_text` から
//! 戻った後に呼ばれる (同期で読める環境でもその場では呼ばない)。

use naui_core::{Result, Tasks};
use windows::ApplicationModel::DataTransfer::{
    Clipboard as WinClipboard, DataPackage, StandardDataFormats,
};
use windows_core::HSTRING;

use crate::to_error;

/// クリップボード (`Windows.ApplicationModel.DataTransfer.Clipboard`)。
/// [`Ui::clipboard`](crate::Ui::clipboard) で取る。
#[derive(Clone)]
pub struct Clipboard {
    tasks: Tasks,
}

impl Clipboard {
    pub(crate) fn new(tasks: Tasks) -> Self {
        Self { tasks }
    }

    /// 文字を書き込む。前の中身は消える。
    ///
    /// アプリを閉じた後も残るよう、書き込んだらすぐ `Flush` する。
    pub fn set_text(&self, text: &str) -> Result<()> {
        let package = DataPackage::new().map_err(|e| to_error("クリップボードの中身の生成", e))?;
        package
            .SetText(&HSTRING::from(text))
            .map_err(|e| to_error("クリップボードの中身の設定", e))?;
        WinClipboard::SetContent(&package)
            .map_err(|e| to_error("クリップボードへの書き込み", e))?;
        let _ = WinClipboard::Flush();
        Ok(())
    }

    /// 文字を読む。文字が入っていなければ `None` を渡す。
    pub fn read_text(&self, f: impl FnOnce(Option<String>) + 'static) {
        self.tasks.spawn(async move {
            f(read().await);
        });
    }
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
