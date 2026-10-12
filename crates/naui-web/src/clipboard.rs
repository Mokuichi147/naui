//! クリップボードの文字の読み書き。
//!
//! 読むほうは環境によって非同期 (GTK4・WinRT・ブラウザ)、書くほうもブラウザでは
//! 非同期なので、4 環境とも読み書きの結果をコールバックで返す。コールバックは
//! UI スレッドで、`read_text` / `set_text` から戻った後に呼ばれる (同期で済む
//! 環境でもその場では呼ばない)。
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

    /// 文字を書き込む。前の中身は消える。書き込めたかどうかを `f` へ渡す。
    ///
    /// ブラウザは利用者の操作 (クリックなど) の中でしか書き込ませないことが
    /// あり、許可の無い `<iframe>` の中などでは断る。断られたときは `Err` が
    /// 届くので、ボタンの `on_click` などから呼び、結果を見てから「コピー
    /// しました」のように知らせる。
    pub fn set_text(&self, text: &str, f: impl FnOnce(Result<()>) + 'static) {
        let promise = Self::native().map(|clipboard| clipboard.write_text(text));
        js_sys::futures::spawn_local(async move {
            let result = match promise {
                Some(promise) => js_sys::futures::JsFuture::from(promise)
                    .await
                    .map(|_| ())
                    .map_err(|e| crate::to_error("クリップボードへの書き込み", e)),
                None => Err(Error::new(
                    "クリップボードへの書き込み",
                    "Clipboard API がありません",
                )),
            };
            f(result);
        });
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
