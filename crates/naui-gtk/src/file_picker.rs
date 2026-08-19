//! ファイルとフォルダーの選択 (`GtkButton` + `GtkFileDialog`)。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;
use naui_core::{FileEntry, FileFilter, FilePickerMode};

use crate::bin::SizeBin;
use crate::callback::FileNotifier;
use crate::widgets::{impl_widget, Widget};

struct FilePickerInner {
    native: gtk::Button,
    bin: SizeBin,
    mode: Cell<FilePickerMode>,
    filters: RefCell<Vec<FileFilter>>,
    selection: RefCell<Vec<FileEntry>>,
    on_select: FileNotifier,
}

/// ファイルやフォルダーを選ばせるボタン。押すと `GtkFileDialog` が出る。
#[derive(Clone)]
pub struct FilePicker(Rc<FilePickerInner>);
impl_widget!(FilePicker);

impl FilePicker {
    pub(crate) fn new(text: &str) -> Self {
        let native = gtk::Button::with_label(text);
        let bin = SizeBin::wrap(&native);
        let picker = Self(Rc::new(FilePickerInner {
            native,
            bin,
            mode: Cell::new(FilePickerMode::default()),
            filters: RefCell::new(Vec::new()),
            selection: RefCell::new(Vec::new()),
            on_select: FileNotifier::default(),
        }));
        {
            let weak = Rc::downgrade(&picker.0);
            picker.0.native.connect_clicked(move |_| {
                if let Some(inner) = weak.upgrade() {
                    FilePicker(inner).open();
                }
            });
        }
        picker
    }

    pub fn set_text(&self, text: &str) {
        self.0.native.set_label(text);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// 何を選ばせるか。既定は「ファイルを 1 つ」。
    pub fn set_mode(&self, mode: FilePickerMode) {
        self.0.mode.set(mode);
    }

    pub fn mode(&self) -> FilePickerMode {
        self.0.mode.get()
    }

    /// 拡張子による絞り込み。フォルダーを選ぶモードでは使われない。
    pub fn set_filters(&self, filters: &[FileFilter]) {
        let mut stored = self.0.filters.borrow_mut();
        stored.clear();
        stored.extend_from_slice(filters);
    }

    /// 直前に選ばれたもの。
    pub fn selection(&self) -> Vec<FileEntry> {
        self.0.selection.borrow().clone()
    }

    /// 選ばれるたびに、選ばれたものの並びで呼ばれる。
    pub fn on_select(&self, f: impl FnMut(&[FileEntry]) + 'static) {
        self.0.on_select.set(f);
    }

    /// ダイアログを出す。ボタンを押したときと同じ。
    pub fn open(&self) {
        let dialog = gtk::FileDialog::new();
        let mode = self.0.mode.get();
        if !mode.is_folder() {
            if let Some(filters) = self.build_filters() {
                dialog.set_filters(Some(&filters));
            }
        }
        let parent = self
            .0
            .native
            .root()
            .and_then(|root| root.downcast::<gtk::Window>().ok());

        let weak = Rc::downgrade(&self.0);
        let finish = move |entries: Vec<FileEntry>| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            *inner.selection.borrow_mut() = entries.clone();
            inner.on_select.emit(&entries);
        };

        match mode {
            FilePickerMode::File => {
                dialog.open(parent.as_ref(), gio::Cancellable::NONE, move |result| {
                    finish(result.ok().iter().filter_map(to_entry).collect());
                });
            }
            FilePickerMode::Files => {
                dialog.open_multiple(parent.as_ref(), gio::Cancellable::NONE, move |result| {
                    let entries = match result {
                        Ok(files) => files
                            .into_iter()
                            .filter_map(|object| object.ok())
                            .filter_map(|object| object.downcast::<gio::File>().ok())
                            .filter_map(|file| to_entry(&file))
                            .collect(),
                        Err(_) => Vec::new(),
                    };
                    finish(entries);
                });
            }
            FilePickerMode::Folder => {
                dialog.select_folder(parent.as_ref(), gio::Cancellable::NONE, move |result| {
                    finish(result.ok().iter().filter_map(to_entry).collect());
                });
            }
        }
    }

    /// 絞り込みを `GtkFileDialog` が要求する形 (`GListModel`) に組み立てる。
    fn build_filters(&self) -> Option<gio::ListStore> {
        let filters = self.0.filters.borrow();
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
}

/// 選ばれた `GFile` を naui の形へ写す。パスを持たないものは捨てる。
fn to_entry(file: &gio::File) -> Option<FileEntry> {
    file.path().map(FileEntry::from_path)
}
