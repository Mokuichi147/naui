use crate::sys;
use naui_core::{FileEntry, FileFilter, FilePickerMode};

/// A button for picking files or folders. Clicking it shows a `GtkFileDialog`.
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

    /// What to let the user pick. The default is "one file".
    #[inline]
    pub fn set_mode(&self, mode: FilePickerMode) {
        self.0.set_mode(mode)
    }

    #[inline]
    pub fn mode(&self) -> FilePickerMode {
        self.0.mode()
    }

    /// Filters by file extension. Not used in folder mode.
    #[inline]
    pub fn set_filters(&self, filters: &[FileFilter]) {
        self.0.set_filters(filters)
    }

    /// What was picked last.
    #[inline]
    pub fn selection(&self) -> Vec<FileEntry> {
        self.0.selection()
    }

    /// Called with the sequence of picked items every time something is picked.
    #[inline]
    pub fn on_select(&self, f: impl FnMut(&[FileEntry]) + 'static) {
        self.0.on_select(f)
    }

    /// Shows the dialog. The same as clicking the button.
    #[inline]
    pub fn open(&self) {
        self.0.open()
    }
}
