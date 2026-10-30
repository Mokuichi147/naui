//! Saving files (`GtkButton` + `GtkFileDialog`'s save).
//!
//! The counterpart of [`crate::FilePicker`]: it writes instead of opening.
//! `GtkFileDialog` lets the user choose the location, and naui writes the bytes from
//! [`FileSaver::set_contents`] to it.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use naui_core::{with_default_extension, Error, FileEntry, FileFilter, Result};

use crate::bin::SizeBin;
use crate::callback::{ErrorNotifier, SavedNotifier};
use crate::widgets::{impl_widget, Widget};

struct FileSaverInner {
    native: gtk::Button,
    bin: SizeBin,
    file_name: RefCell<String>,
    filters: RefCell<Vec<FileFilter>>,
    contents: RefCell<Vec<u8>>,
    destination: RefCell<Option<FileEntry>>,
    on_save: SavedNotifier,
    on_error: ErrorNotifier,
}

/// A button that lets the user write contents to a file. Clicking it shows the
/// save mode of `GtkFileDialog`.
#[derive(Clone)]
pub struct FileSaver(Rc<FileSaverInner>);
impl_widget!(FileSaver);

impl FileSaver {
    pub(crate) fn new(text: &str) -> Self {
        let native = gtk::Button::with_label(text);
        let bin = SizeBin::wrap(&native);
        let saver = Self(Rc::new(FileSaverInner {
            native,
            bin,
            file_name: RefCell::new(String::new()),
            filters: RefCell::new(Vec::new()),
            contents: RefCell::new(Vec::new()),
            destination: RefCell::new(None),
            on_save: SavedNotifier::default(),
            on_error: ErrorNotifier::default(),
        }));
        {
            let weak = Rc::downgrade(&saver.0);
            saver.0.native.connect_clicked(move |_| {
                if let Some(inner) = weak.upgrade() {
                    FileSaver(inner).open();
                }
            });
        }
        saver
    }

    pub fn set_text(&self, text: &str) {
        self.0.native.set_label(text);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// The name prefilled in the dialog. If empty, GTK's default is used.
    pub fn set_file_name(&self, name: &str) {
        *self.0.file_name.borrow_mut() = name.to_string();
    }

    pub fn file_name(&self) -> String {
        self.0.file_name.borrow().clone()
    }

    /// Filters by file type. The first extension becomes the default extension.
    pub fn set_filters(&self, filters: &[FileFilter]) {
        let mut stored = self.0.filters.borrow_mut();
        stored.clear();
        stored.extend_from_slice(filters);
    }

    /// The contents to write. Every save writes exactly these bytes.
    pub fn set_contents(&self, contents: &[u8]) {
        *self.0.contents.borrow_mut() = contents.to_vec();
    }

    /// The size of the contents to write (in bytes).
    pub fn contents_len(&self) -> usize {
        self.0.contents.borrow().len()
    }

    /// Where the contents were last written. `None` if nothing has been saved yet.
    pub fn destination(&self) -> Option<FileEntry> {
        self.0.destination.borrow().clone()
    }

    /// Called when writing succeeds. Not called when the user cancels.
    pub fn on_save(&self, f: impl FnMut(&FileEntry) + 'static) {
        self.0.on_save.set(f);
    }

    /// Called when writing fails (no write permission, for example).
    pub fn on_error(&self, f: impl FnMut(&Error) + 'static) {
        self.0.on_error.set(f);
    }

    /// Shows the dialog. The same as clicking the button.
    pub fn open(&self) {
        let dialog = gtk::FileDialog::new();
        {
            let filters = self.0.filters.borrow();
            let name = with_default_extension(&self.0.file_name.borrow(), &filters);
            if !name.is_empty() {
                dialog.set_initial_name(Some(&name));
            }
            if let Some(store) = build_filters(&filters) {
                dialog.set_filters(Some(&store));
            }
        }
        let parent = self
            .0
            .native
            .root()
            .and_then(|root| root.downcast::<gtk::Window>().ok());

        let weak = Rc::downgrade(&self.0);
        dialog.save(parent.as_ref(), gio::Cancellable::NONE, move |result| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            FileSaver(inner).finish(result);
        });
    }

    /// Receives the dialog's result, then writes and notifies.
    fn finish(&self, result: std::result::Result<gio::File, glib::Error>) {
        let file = match result {
            Ok(file) => file,
            // 取り消しは失敗として扱わない (他のバックエンドと同じ)。
            Err(e) if is_cancelled(&e) => return,
            Err(e) => {
                self.0
                    .on_error
                    .emit(&Error::new("show the save dialog", e.to_string()));
                return;
            }
        };
        let Some(path) = file.path() else {
            self.0.on_error.emit(&Error::new(
                "get the save location",
                "could not get the path",
            ));
            return;
        };
        // 通知の中からこのウィジェットを触れるよう、借用を手放してから呼ぶ。
        let contents = self.0.contents.borrow().clone();
        match write_contents(&path, &contents) {
            Ok(entry) => {
                *self.0.destination.borrow_mut() = Some(entry.clone());
                self.0.on_save.emit(&entry);
            }
            Err(e) => self.0.on_error.emit(&e),
        }
    }
}

/// Builds the filters in the form `GtkFileDialog` requires (`GListModel`).
fn build_filters(filters: &[FileFilter]) -> Option<gio::ListStore> {
    let usable: Vec<&FileFilter> = filters.iter().filter(|f| !f.is_empty()).collect();
    if usable.is_empty() {
        return None;
    }
    let store = gio::ListStore::new::<gtk::FileFilter>();
    for filter in usable {
        let native = gtk::FileFilter::new();
        native.set_name(Some(filter.label()));
        for extension in filter.extensions() {
            native.add_suffix(extension);
        }
        store.append(&native);
    }
    Some(store)
}

/// Whether the user merely closed (cancelled) the dialog.
fn is_cancelled(error: &glib::Error) -> bool {
    error.matches(gtk::DialogError::Dismissed)
        || error.matches(gtk::DialogError::Cancelled)
        || error.matches(gio::IOErrorEnum::Cancelled)
}

/// Writes to the chosen location and returns a [`FileEntry`] pointing at it.
fn write_contents(path: &Path, contents: &[u8]) -> Result<FileEntry> {
    std::fs::write(path, contents).map_err(|e| Error::new("write the file", e.to_string()))?;
    Ok(FileEntry::from_path(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contents_are_written_to_the_chosen_path() {
        let path = std::env::temp_dir().join("naui-file-saver-テスト.txt");
        let _ = std::fs::remove_file(&path);

        let entry = write_contents(&path, "こんにちは".as_bytes()).expect("書き出せること");
        assert_eq!(entry.path(), Some(path.as_path()));
        assert_eq!(std::fs::read(&path).unwrap(), "こんにちは".as_bytes());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_failed_write_is_reported_as_an_error() {
        let path = std::env::temp_dir()
            .join("naui-無いフォルダー")
            .join("a.txt");
        let error = write_contents(&path, b"x").expect_err("失敗すること");
        assert_eq!(error.context(), "write the file");
    }

    #[test]
    fn empty_filters_do_not_build_a_list() {
        assert!(build_filters(&[]).is_none());
        assert!(build_filters(&[FileFilter::new("空", [] as [&str; 0])]).is_none());
    }
}
