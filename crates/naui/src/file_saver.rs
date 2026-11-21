use crate::sys;
use naui_core::{Error, FileEntry, FileFilter};

/// A button that lets the user write contents to a file. Clicking it shows the
/// save mode of `GtkFileDialog`.
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

    /// The name prefilled in the dialog. If empty, GTK's default is used.
    #[inline]
    pub fn set_file_name(&self, name: &str) {
        self.0.set_file_name(name)
    }

    #[inline]
    pub fn file_name(&self) -> String {
        self.0.file_name()
    }

    /// Filters by file type. The first extension becomes the default extension.
    #[inline]
    pub fn set_filters(&self, filters: &[FileFilter]) {
        self.0.set_filters(filters)
    }

    /// The contents to write. Every save writes exactly these bytes.
    #[inline]
    pub fn set_contents(&self, contents: &[u8]) {
        self.0.set_contents(contents)
    }

    /// The size of the contents to write (in bytes).
    #[inline]
    pub fn contents_len(&self) -> usize {
        self.0.contents_len()
    }

    /// Where the contents were last written. `None` if nothing has been saved yet.
    #[inline]
    pub fn destination(&self) -> Option<FileEntry> {
        self.0.destination()
    }

    /// Called when writing succeeds. Not called when the user cancels.
    #[inline]
    pub fn on_save(&self, f: impl FnMut(&FileEntry) + 'static) {
        self.0.on_save(f)
    }

    /// Called when writing fails (no write permission, for example).
    #[inline]
    pub fn on_error(&self, f: impl FnMut(&Error) + 'static) {
        self.0.on_error(f)
    }

    /// Shows the dialog. The same as clicking the button.
    #[inline]
    pub fn open(&self) {
        self.0.open()
    }

    /// Saves as if the user had clicked the button and chosen `destination` in the
    /// dialog, without showing it. Natively the contents are written to
    /// `destination`'s path, and [`on_save`](Self::on_save) is called with it (or
    /// [`on_error`](Self::on_error) if writing fails or it has no path). Does
    /// nothing while disabled.
    ///
    /// **On the Web nothing is written**: a page cannot write to a path, and a test
    /// should not start a download. `on_save` is called with an entry carrying only
    /// `destination`'s name, as after a download.
    ///
    /// For tests and automation. Call it instead of clicking the button: a click
    /// shows the real dialog, which a test cannot answer. See
    /// [Testing apps](crate::guide#testing-apps).
    #[inline]
    pub fn choose(&self, destination: &FileEntry) {
        self.0.choose(destination)
    }
}
