use crate::sys;
use naui_core::{Error, FileEntry, FileFilter};

/// 内容をファイルへ書き出させるボタン。押すと `GtkFileDialog` の保存が出る。
#[derive(Clone)]
pub struct FileSaver(pub(crate) sys::FileSaver);

impl_widget!(FileSaver);

impl FileSaver {
    #[inline]
    pub fn set_text(&self, text: &str) {
        self.0.set_text(text)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// ダイアログに最初から入れておく名前。空なら GTK の既定に任せる。
    #[inline]
    pub fn set_file_name(&self, name: &str) {
        self.0.set_file_name(name)
    }

    #[inline]
    pub fn file_name(&self) -> String {
        self.0.file_name()
    }

    /// 種類の絞り込み。先頭の拡張子が既定の拡張子になる。
    #[inline]
    pub fn set_filters(&self, filters: &[FileFilter]) {
        self.0.set_filters(filters)
    }

    /// 書き出す内容。保存のたびに、このバイト列がそのまま書かれる。
    #[inline]
    pub fn set_contents(&self, contents: &[u8]) {
        self.0.set_contents(contents)
    }

    /// 書き出す内容の大きさ (バイト数)。
    #[inline]
    pub fn contents_len(&self) -> usize {
        self.0.contents_len()
    }

    /// 最後に書き出した先。まだ保存していなければ `None`。
    #[inline]
    pub fn destination(&self) -> Option<FileEntry> {
        self.0.destination()
    }

    /// 書き出しに成功したときに呼ばれる。取り消したときは呼ばれない。
    #[inline]
    pub fn on_save(&self, f: impl FnMut(&FileEntry) + 'static) {
        self.0.on_save(f)
    }

    /// 書き出しに失敗したときに呼ばれる (書き込み権限が無い、など)。
    #[inline]
    pub fn on_error(&self, f: impl FnMut(&Error) + 'static) {
        self.0.on_error(f)
    }

    /// ダイアログを出す。ボタンを押したときと同じ。
    #[inline]
    pub fn open(&self) {
        self.0.open()
    }
}
