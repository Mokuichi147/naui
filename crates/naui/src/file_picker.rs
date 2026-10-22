use crate::sys;
use naui_core::{FileEntry, FileFilter, FilePickerMode};

/// ファイルやフォルダーを選ばせるボタン。押すと `GtkFileDialog` が出る。
#[derive(Clone)]
pub struct FilePicker(pub(crate) sys::FilePicker);

impl_widget!(FilePicker);

impl FilePicker {
    #[inline]
    pub fn set_text(&self, text: &str) {
        self.0.set_text(text)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// 何を選ばせるか。既定は「ファイルを 1 つ」。
    #[inline]
    pub fn set_mode(&self, mode: FilePickerMode) {
        self.0.set_mode(mode)
    }

    #[inline]
    pub fn mode(&self) -> FilePickerMode {
        self.0.mode()
    }

    /// 拡張子による絞り込み。フォルダーを選ぶモードでは使われない。
    #[inline]
    pub fn set_filters(&self, filters: &[FileFilter]) {
        self.0.set_filters(filters)
    }

    /// 直前に選ばれたもの。
    #[inline]
    pub fn selection(&self) -> Vec<FileEntry> {
        self.0.selection()
    }

    /// 選ばれるたびに、選ばれたものの並びで呼ばれる。
    #[inline]
    pub fn on_select(&self, f: impl FnMut(&[FileEntry]) + 'static) {
        self.0.on_select(f)
    }

    /// ダイアログを出す。ボタンを押したときと同じ。
    #[inline]
    pub fn open(&self) {
        self.0.open()
    }
}
