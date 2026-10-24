//! File / folder picking (AppKit).
//!
//! AppKit has no "control that picks and shows the result". A push button
//! (`NSButton`) is placed, and clicking it shows an **`NSOpenPanel`** app-modally.
//! The dialog itself is AppKit's; the list, search, sidebar and access permissions
//! are all handled by AppKit.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::{FileEntry, FileFilter, FilePickerMode};
use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker, Message};
use objc2_app_kit::{NSButton, NSModalResponseOK, NSOpenPanel, NSView};
use objc2_foundation::{NSArray, NSString};

use crate::trampoline::ActionTarget;
use crate::widgets::{impl_widget, Widget};

/// The handler notified when something is picked. Shares a single replaceable
/// closure.
///
/// So that usage such as opening the dialog again from inside the notification does
/// not double-borrow, the closure is taken out only for the duration of the call
/// (the same design as [`crate::trampoline::SelectHandler`]).
#[derive(Clone, Default)]
struct SelectionHandler(Rc<RefCell<Option<Box<dyn FnMut(&[FileEntry])>>>>);

impl SelectionHandler {
    fn set(&self, f: impl FnMut(&[FileEntry]) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, entries: &[FileEntry]) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(entries);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

struct FilePickerInner {
    native: Retained<NSButton>,
    target: RefCell<Option<Retained<ActionTarget>>>,
    mode: Cell<FilePickerMode>,
    filters: RefCell<Vec<FileFilter>>,
    selection: RefCell<Vec<FileEntry>>,
    on_select: SelectionHandler,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PanelOptions {
    mode: FilePickerMode,
    extensions: Vec<String>,
}

impl PanelOptions {
    fn new(mode: FilePickerMode, filters: &[FileFilter]) -> Self {
        let extensions = filters
            .iter()
            .flat_map(|filter| filter.extensions().iter().cloned())
            .collect();
        Self { mode, extensions }
    }
}

/// A button for picking files or folders (NSButton + NSOpenPanel).
#[derive(Clone)]
pub struct FilePicker(Rc<FilePickerInner>);
impl_widget!(FilePicker);

impl FilePicker {
    pub(crate) fn new(mtm: MainThreadMarker, text: &str) -> Self {
        let native = unsafe {
            NSButton::buttonWithTitle_target_action(&NSString::from_str(text), None, None, mtm)
        };
        let this = Self(Rc::new(FilePickerInner {
            native,
            target: RefCell::new(None),
            mode: Cell::new(FilePickerMode::default()),
            filters: RefCell::new(Vec::new()),
            selection: RefCell::new(Vec::new()),
            on_select: SelectionHandler::default(),
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
        let weak: Weak<FilePickerInner> = Rc::downgrade(&self.0);
        let target = ActionTarget::new(mtm, move || {
            if let Some(inner) = weak.upgrade() {
                FilePicker(inner).open();
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

    /// Decides what to let the user pick (one file by default).
    pub fn set_mode(&self, mode: FilePickerMode) {
        self.0.mode.set(mode);
    }

    pub fn mode(&self) -> FilePickerMode {
        self.0.mode.get()
    }

    /// Filters by extension. Ignored for [`FilePickerMode::Folder`].
    pub fn set_filters(&self, filters: &[FileFilter]) {
        *self.0.filters.borrow_mut() = filters.to_vec();
    }

    /// What was picked last. Empty if nothing has been picked yet.
    pub fn selection(&self) -> Vec<FileEntry> {
        self.0.selection.borrow().clone()
    }

    /// Called when something is picked. Not called when the user cancels. Setting it
    /// again removes the previous one.
    pub fn on_select(&self, f: impl FnMut(&[FileEntry]) + 'static) {
        self.0.on_select.set(f);
    }

    /// Shows the dialog. Clicking the button calls the same thing.
    ///
    /// `NSOpenPanel` is app-modal, so this does not return until it is closed.
    pub fn open(&self) {
        let Some(entries) = self.run_panel() else {
            return; // 取り消された。
        };
        *self.0.selection.borrow_mut() = entries.clone();
        self.0.on_select.emit(&entries);
    }

    /// Builds and returns an `NSOpenPanel` reflecting the current settings. **It is not
    /// shown yet.**
    ///
    /// A backend-specific escape hatch. AppKit-specific uses, such as showing it as a
    /// sheet (`beginSheetModalForWindow:`) or setting the starting directory, start
    /// from here.
    pub fn native_panel(&self) -> Retained<NSOpenPanel> {
        let mtm = MainThreadMarker::from(&*self.0.native);
        let panel = NSOpenPanel::openPanel(mtm);
        let options = PanelOptions::new(self.0.mode.get(), &self.0.filters.borrow());
        panel.setCanChooseFiles(!options.mode.is_folder());
        panel.setCanChooseDirectories(options.mode.is_folder());
        panel.setAllowsMultipleSelection(options.mode.allows_multiple());

        if !options.mode.is_folder() {
            let extensions: Vec<Retained<NSString>> = options
                .extensions
                .iter()
                .map(|extension| NSString::from_str(extension))
                .collect();
            if !extensions.is_empty() {
                let types = NSArray::from_retained_slice(&extensions);
                // `allowedContentTypes` は UTType (objc2-uniform-type-identifiers)
                // を要求する。拡張子だけのために依存を増やしたくないので、
                // 非推奨だが同じことができる旧 API を使う。
                #[allow(deprecated)]
                panel.setAllowedFileTypes(Some(&types));
            }
        }
        panel
    }

    /// Shows the panel and maps the chosen URLs to paths. `None` if cancelled.
    fn run_panel(&self) -> Option<Vec<FileEntry>> {
        let panel = self.native_panel();
        if panel.runModal() != NSModalResponseOK {
            return None;
        }
        let entries: Vec<FileEntry> = panel
            .URLs()
            .iter()
            .filter_map(|url| url.path().map(|p| FileEntry::from_path(p.to_string())))
            .collect();
        // 何も取れなかったときは、取り消しと同じ扱いにする (他のバックエンドと同じ)。
        if entries.is_empty() {
            return None;
        }
        Some(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_options_flatten_and_normalize_filters() {
        let filters = [
            FileFilter::new("画像", ["*.PNG", "jpg"]),
            FileFilter::new("文書", ["txt"]),
        ];
        let options = PanelOptions::new(FilePickerMode::File, &filters);

        assert_eq!(options.mode, FilePickerMode::File);
        assert_eq!(options.extensions, ["png", "jpg", "txt"]);
    }
}
