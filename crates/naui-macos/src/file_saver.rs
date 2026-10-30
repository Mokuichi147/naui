//! Saving files (AppKit).
//!
//! As with [`crate::FilePicker`], AppKit has no "control for saving". A push button
//! (`NSButton`) is placed, and clicking it shows an **`NSSavePanel`** app-modally.
//! The location list, new folders and overwrite confirmation are all handled by
//! AppKit.
//!
//! Writing to the chosen location is done by naui, which `std::fs::write`s the
//! bytes passed with [`FileSaver::set_contents`] as is.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use naui_core::{with_default_extension, Error, FileEntry, FileFilter, Result};
use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker, Message};
use objc2_app_kit::{NSButton, NSModalResponseOK, NSSavePanel, NSView};
use objc2_foundation::{NSArray, NSString};

use crate::trampoline::ActionTarget;
use crate::widgets::{impl_widget, Widget};

/// A single replaceable closure.
///
/// So that usage such as saving again from inside the notification does not
/// double-borrow, the closure is taken out only for the duration of the call (the
/// same design as `FilePicker`).
struct Handler<T: ?Sized>(Rc<RefCell<Option<Box<dyn FnMut(&T)>>>>);

impl<T: ?Sized> Default for Handler<T> {
    fn default() -> Self {
        Self(Rc::new(RefCell::new(None)))
    }
}

impl<T: ?Sized> Handler<T> {
    fn set(&self, f: impl FnMut(&T) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, value: &T) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(value);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

struct FileSaverInner {
    native: Retained<NSButton>,
    target: RefCell<Option<Retained<ActionTarget>>>,
    file_name: RefCell<String>,
    filters: RefCell<Vec<FileFilter>>,
    contents: RefCell<Vec<u8>>,
    destination: RefCell<Option<FileEntry>>,
    on_save: Handler<FileEntry>,
    on_error: Handler<Error>,
}

/// A button that lets the user write contents to a file (NSButton + NSSavePanel).
#[derive(Clone)]
pub struct FileSaver(Rc<FileSaverInner>);
impl_widget!(FileSaver);

impl FileSaver {
    pub(crate) fn new(mtm: MainThreadMarker, text: &str) -> Self {
        let native = unsafe {
            NSButton::buttonWithTitle_target_action(&NSString::from_str(text), None, None, mtm)
        };
        let this = Self(Rc::new(FileSaverInner {
            native,
            target: RefCell::new(None),
            file_name: RefCell::new(String::new()),
            filters: RefCell::new(Vec::new()),
            contents: RefCell::new(Vec::new()),
            destination: RefCell::new(None),
            on_save: Handler::default(),
            on_error: Handler::default(),
        }));
        this.install_click_handler();
        this
    }

    /// Makes clicking the button show the dialog.
    ///
    /// The trampoline lives inside the handle, so capturing it strongly would create a
    /// cycle that is never freed. It is held weakly.
    fn install_click_handler(&self) {
        let mtm = MainThreadMarker::from(&*self.0.native);
        let weak: Weak<FileSaverInner> = Rc::downgrade(&self.0);
        let target = ActionTarget::new(mtm, move || {
            if let Some(inner) = weak.upgrade() {
                FileSaver(inner).open();
            }
        });
        unsafe {
            self.0.native.setTarget(Some(&target));
            self.0.native.setAction(Some(sel!(invoke:)));
        }
        *self.0.target.borrow_mut() = Some(target);
    }

    pub fn set_text(&self, text: &str) {
        self.0.native.setTitle(&NSString::from_str(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.setEnabled(enabled);
    }

    /// The name prefilled in the dialog. If empty, AppKit's default is used.
    pub fn set_file_name(&self, name: &str) {
        *self.0.file_name.borrow_mut() = name.to_string();
    }

    pub fn file_name(&self) -> String {
        self.0.file_name.borrow().clone()
    }

    /// Filters by file type. The first extension becomes the default extension.
    pub fn set_filters(&self, filters: &[FileFilter]) {
        *self.0.filters.borrow_mut() = filters.to_vec();
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

    /// Called when writing succeeds. Not called when the user cancels. Setting it again
    /// removes the previous one.
    pub fn on_save(&self, f: impl FnMut(&FileEntry) + 'static) {
        self.0.on_save.set(f);
    }

    /// Called when writing fails (no write permission, for example).
    pub fn on_error(&self, f: impl FnMut(&Error) + 'static) {
        self.0.on_error.set(f);
    }

    /// Shows the dialog. Clicking the button calls the same thing.
    ///
    /// `NSSavePanel` is app-modal, so this does not return until it is closed.
    pub fn open(&self) {
        let panel = self.native_panel();
        if panel.runModal() != NSModalResponseOK {
            return; // 取り消された。
        }
        let Some(path) = panel
            .URL()
            .and_then(|url| url.path())
            .map(|path| PathBuf::from(path.to_string()))
        else {
            self.0.on_error.emit(&Error::new(
                "get the save location",
                "could not get the path",
            ));
            return;
        };
        self.write_to(&path);
    }

    /// Builds and returns an `NSSavePanel` reflecting the current settings. **It is not
    /// shown yet.**
    ///
    /// A backend-specific escape hatch. AppKit-specific uses, such as showing it as a
    /// sheet (`beginSheetModalForWindow:`) or setting the starting directory, start
    /// from here.
    pub fn native_panel(&self) -> Retained<NSSavePanel> {
        let mtm = MainThreadMarker::from(&*self.0.native);
        let panel = NSSavePanel::savePanel(mtm);
        panel.setCanCreateDirectories(true);

        let filters = self.0.filters.borrow();
        let name = with_default_extension(&self.0.file_name.borrow(), &filters);
        if !name.is_empty() {
            panel.setNameFieldStringValue(&NSString::from_str(&name));
        }

        let extensions: Vec<Retained<NSString>> = filters
            .iter()
            .flat_map(|filter| filter.extensions())
            .map(|extension| NSString::from_str(extension))
            .collect();
        if !extensions.is_empty() {
            let types = NSArray::from_retained_slice(&extensions);
            // `allowedContentTypes` は UTType (objc2-uniform-type-identifiers)
            // を要求する。拡張子だけのために依存を増やしたくないので、
            // 非推奨だが同じことができる旧 API を使う (`FilePicker` と同じ)。
            #[allow(deprecated)]
            panel.setAllowedFileTypes(Some(&types));
        }
        panel
    }

    /// Writes to the chosen location and notifies the result.
    ///
    /// The borrow is released before calling, so the notification can touch this
    /// widget.
    fn write_to(&self, path: &Path) {
        let contents = self.0.contents.borrow().clone();
        match write_contents(path, &contents) {
            Ok(entry) => {
                *self.0.destination.borrow_mut() = Some(entry.clone());
                self.0.on_save.emit(&entry);
            }
            Err(e) => self.0.on_error.emit(&e),
        }
    }
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
        assert_eq!(entry.name(), "naui-file-saver-テスト.txt");
        assert_eq!(std::fs::read(&path).unwrap(), "こんにちは".as_bytes());

        // 2 回目は上書きになる (ダイアログ側で確認済みのため)。
        write_contents(&path, b"").expect("上書きできること");
        assert!(std::fs::read(&path).unwrap().is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_failed_write_is_reported_as_an_error() {
        // 存在しないフォルダーの下へは書けない。
        let path = std::env::temp_dir()
            .join("naui-無いフォルダー")
            .join("a.txt");
        let error = write_contents(&path, b"x").expect_err("失敗すること");
        assert_eq!(error.context(), "write the file");
        assert!(!error.detail().is_empty());
    }
}
