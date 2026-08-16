//! ファイル / フォルダー選択 (AppKit)。
//!
//! AppKit に「選択して結果を表示するコントロール」は無い。押しボタン
//! (`NSButton`) を置き、押されたら **`NSOpenPanel`** をアプリモーダルで出す。
//! ダイアログそのものは AppKit のもので、一覧・検索・サイドバー・
//! アクセス権限の扱いはすべて AppKit が行う。

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use naui_core::{FileEntry, FileFilter, FilePickerMode};
use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker, Message};
use objc2_app_kit::{NSButton, NSModalResponseOK, NSOpenPanel, NSView};
use objc2_foundation::{NSArray, NSString};

use crate::trampoline::ActionTarget;
use crate::widgets::{impl_widget, Widget};

/// 選択されたときの通知先。差し替え可能な 1 本のクロージャを共有で持つ。
///
/// 通知の中からもう一度ダイアログを開くような使い方をしても二重借用に
/// ならないよう、呼び出しの間だけクロージャを取り出す
/// ([`crate::trampoline::SelectHandler`] と同じ作り)。
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

/// ファイルやフォルダーを選ばせるボタン (NSButton + NSOpenPanel)。
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

    /// ボタンを押したらダイアログが出るようにする。
    ///
    /// トランポリンはハンドルの中にあるので、強参照で捕まえると
    /// 循環して解放されない。弱参照にしておく。
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

    /// 何を選ばせるかを決める (既定はファイル 1 つ)。
    pub fn set_mode(&self, mode: FilePickerMode) {
        self.0.mode.set(mode);
    }

    pub fn mode(&self) -> FilePickerMode {
        self.0.mode.get()
    }

    /// 拡張子で絞り込む。[`FilePickerMode::Folder`] のときは無視される。
    pub fn set_filters(&self, filters: &[FileFilter]) {
        *self.0.filters.borrow_mut() = filters.to_vec();
    }

    /// 最後に選ばれたもの。まだ選ばれていなければ空。
    pub fn selection(&self) -> Vec<FileEntry> {
        self.0.selection.borrow().clone()
    }

    /// 選ばれたときに呼ばれる。取り消したときは呼ばれない。
    /// 設定し直すと以前のものは外れる。
    pub fn on_select(&self, f: impl FnMut(&[FileEntry]) + 'static) {
        self.0.on_select.set(f);
    }

    /// ダイアログを出す。ボタンを押したときにも同じものが呼ばれる。
    ///
    /// `NSOpenPanel` はアプリモーダルなので、閉じられるまで戻らない。
    pub fn open(&self) {
        let Some(entries) = self.run_panel() else {
            return; // 取り消された。
        };
        *self.0.selection.borrow_mut() = entries.clone();
        self.0.on_select.emit(&entries);
    }

    /// いまの設定を反映した `NSOpenPanel` を組み立てて返す。**まだ表示しない。**
    ///
    /// バックエンド固有の脱出口。シートとして出したい (`beginSheetModalForWindow:`)、
    /// 開始ディレクトリを指定したい、といった AppKit 固有の使い方はここから行う。
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

    /// パネルを出して、選ばれた URL をパスへ写す。取り消されたら `None`。
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
