use crate::sys;
use naui_core::Result;

/// クリップボード (`GdkClipboard`)。[`Ui::clipboard`](crate::Ui::clipboard) で取る。
#[derive(Clone)]
pub struct Clipboard(pub(crate) sys::Clipboard);

impl_backend!(Clipboard);

impl Clipboard {
    /// 文字を書き込む。前の中身は消える。書き込めたかどうかを `f` へ渡す。
    #[inline]
    pub fn set_text(&self, text: &str, f: impl FnOnce(Result<()>) + 'static) {
        self.0.set_text(text, f)
    }

    /// 文字を読む。文字が入っていなければ `None` を渡す。
    #[inline]
    pub fn read_text(&self, f: impl FnOnce(Option<String>) + 'static) {
        self.0.read_text(f)
    }
}
