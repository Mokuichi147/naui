//! クリップボードの文字の読み書き。
//!
//! 読むほうは環境によって非同期 (GTK4・WinRT・ブラウザ) なので、4 環境とも
//! 結果をコールバックで返す。コールバックは UI スレッドで、`read_text` から
//! 戻った後に呼ばれる (同期で読める環境でもその場では呼ばない)。

use gtk::prelude::*;
use gtk::{gdk, gio};
use naui_core::{Error, Result};

/// クリップボード (`GdkClipboard`)。[`Ui::clipboard`](crate::Ui::clipboard) で取る。
#[derive(Clone)]
pub struct Clipboard;

impl Clipboard {
    fn native() -> Option<gdk::Clipboard> {
        gdk::Display::default().map(|display| display.clipboard())
    }

    /// 文字を書き込む。前の中身は消える。
    pub fn set_text(&self, text: &str) -> Result<()> {
        let clipboard = Self::native()
            .ok_or_else(|| Error::new("クリップボードへの書き込み", "ディスプレイがありません"))?;
        clipboard.set_text(text);
        Ok(())
    }

    /// 文字を読む。文字が入っていなければ `None` を渡す。
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
