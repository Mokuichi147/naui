//! クリップボードの文字の読み書き。
//!
//! 読むほうは環境によって非同期 (GTK4・WinRT・ブラウザ)、書くほうもブラウザでは
//! 非同期なので、4 環境とも読み書きの結果をコールバックで返す。コールバックは
//! UI スレッドで、`read_text` / `set_text` から戻った後に呼ばれる (同期で済む
//! 環境でもその場では呼ばない)。

use naui_core::{Error, Result, Tasks};
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::NSString;

/// クリップボード (`NSPasteboard` の一般用)。[`Ui::clipboard`](crate::Ui::clipboard) で取る。
#[derive(Clone)]
pub struct Clipboard {
    tasks: Tasks,
}

impl Clipboard {
    pub(crate) fn new(tasks: Tasks) -> Self {
        Self { tasks }
    }

    /// 文字を書き込む。前の中身は消える。書き込めたかどうかを `f` へ渡す。
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

    /// 文字を読む。文字が入っていなければ `None` を渡す。
    pub fn read_text(&self, f: impl FnOnce(Option<String>) + 'static) {
        let text = NSPasteboard::generalPasteboard()
            .stringForType(unsafe { NSPasteboardTypeString })
            .map(|text| text.to_string());
        self.tasks.spawn(async move { f(text) });
    }
}
