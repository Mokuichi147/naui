//! クリップボードの文字の読み書き。
//!
//! 読むほうは環境によって非同期 (GTK4・WinRT・ブラウザ) なので、4 環境とも
//! 結果をコールバックで返す。コールバックは UI スレッドで、`read_text` から
//! 戻った後に呼ばれる (同期で読める環境でもその場では呼ばない)。
//!
//! ブラウザの Clipboard API は安全な文脈 (https または localhost) でしか
//! 使えず、読むときは利用者の許可を求められることがある。

use naui_core::{Error, Result};

/// クリップボード (`navigator.clipboard`)。[`Ui::clipboard`](crate::Ui::clipboard) で取る。
#[derive(Clone)]
pub struct Clipboard;

impl Clipboard {
    fn native() -> Option<web_sys::Clipboard> {
        web_sys::window().map(|window| window.navigator().clipboard())
    }

    /// 文字を書き込む。前の中身は消える。
    ///
    /// 書き込みはブラウザが非同期で行うので、ここで分かるのは Clipboard API が
    /// 使えるかどうかだけ。ブラウザは利用者の操作 (クリックなど) の中でしか
    /// 書き込ませないことがあるので、ボタンの `on_click` などから呼ぶ。
    pub fn set_text(&self, text: &str) -> Result<()> {
        let clipboard = Self::native().ok_or_else(|| {
            Error::new("クリップボードへの書き込み", "Clipboard API がありません")
        })?;
        let promise = clipboard.write_text(text);
        js_sys::futures::spawn_local(async move {
            let _ = js_sys::futures::JsFuture::from(promise).await;
        });
        Ok(())
    }

    /// 文字を読む。文字が入っていない・読めなかったときは `None` を渡す。
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
