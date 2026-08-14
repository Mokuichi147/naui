//! ファイル / フォルダー選択 (DOM)。
//!
//! ブラウザで「選ばせる」のは `<input type="file">` そのもので、
//! ダイアログを出すのも一覧を描くのもブラウザ (と OS) が行う。
//! ただし `<input type="file">` のボタン文字列はブラウザ所有で差し替えられない
//! ため、**表に出すのは `<button>`、`<input>` は隠して押しを転送する**という
//! 組み立てにしてある。
//!
//! ## 他の環境との違い
//!
//! - **パスは取れない。** ブラウザは絶対パスを渡さないので
//!   [`FileEntry::path`](miui_core::FileEntry::path) は常に `None` になる。
//! - **ユーザー操作の中でしか開けない。** [`FilePicker::open`] を
//!   クリック等のイベント外から呼ぶと、ブラウザに無視される。
//! - **フォルダーを選ぶと、ブラウザはその中のファイル一覧を返す。**
//!   他の環境はフォルダー 1 つを返すので、`webkitRelativePath` の先頭から
//!   フォルダー名を取り出し、**1 件だけ**返すようにそろえている。
//!   中のファイルが要るときは [`FilePicker::native_element`] から
//!   `<input>` を取り出して `FileList` を読む。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use miui_core::{accept_attribute, FileEntry, FileFilter, FilePickerMode, Result};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, Element, HtmlElement, HtmlInputElement};

use crate::to_error;
use crate::widgets::{create, impl_widget, set_disabled, Listener, Widget};

struct FilePickerInner {
    /// `<span>` に `<button>` と隠した `<input type="file">` を入れたもの。
    element: HtmlElement,
    button: HtmlElement,
    input: HtmlInputElement,
    mode: Cell<FilePickerMode>,
    selection: RefCell<Vec<FileEntry>>,
    on_select: RefCell<Option<Box<dyn FnMut(&[FileEntry])>>>,
    /// ボタンの押しを `<input>` へ転送するもの。
    click: RefCell<Option<Listener>>,
    change: RefCell<Option<Listener>>,
}

/// ファイルやフォルダーを選ばせるボタン (`<button>` + `<input type="file">`)。
#[derive(Clone)]
pub struct FilePicker(Rc<FilePickerInner>);
impl_widget!(FilePicker, element);

impl FilePicker {
    pub(crate) fn new(doc: &Document, text: &str) -> Result<Self> {
        let element: HtmlElement = create(doc, "span")?.unchecked_into();
        let button: HtmlElement = create(doc, "button")?.unchecked_into();
        button.set_text_content(Some(text));
        let input: HtmlInputElement = create(doc, "input")?.unchecked_into();
        input.set_type("file");
        // ブラウザ既定のボタンは出さず、レイアウトからも外す。
        input.set_hidden(true);

        element
            .append_child(&button)
            .map_err(|e| to_error("ファイル選択の組み立て", e))?;
        element
            .append_child(&input)
            .map_err(|e| to_error("ファイル選択の組み立て", e))?;
        let style = element.style();
        let _ = style.set_property("display", "inline-flex");

        let this = Self(Rc::new(FilePickerInner {
            element,
            button,
            input,
            mode: Cell::new(FilePickerMode::default()),
            selection: RefCell::new(Vec::new()),
            on_select: RefCell::new(None),
            click: RefCell::new(None),
            change: RefCell::new(None),
        }));
        this.install_handlers()?;
        Ok(this)
    }

    fn install_handlers(&self) -> Result<()> {
        // ボタンの押しは、そのイベントの中で `<input>` へ転送する。
        // こうするとブラウザから見てユーザー操作のままなので、
        // ファイル選択ダイアログが開く。
        let input = self.0.input.clone();
        let click = Listener::attach(self.0.button.as_ref(), "click", move || {
            input.set_value("");
            input.click();
        })?;
        *self.0.click.borrow_mut() = Some(click);

        let weak = Rc::downgrade(&self.0);
        let change = Listener::attach(self.0.input.as_ref(), "change", move || {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let picker = FilePicker(inner);
            let entries = picker.read_selection();
            if entries.is_empty() {
                return; // 取り消された。
            }
            *picker.0.selection.borrow_mut() = entries.clone();
            picker.emit(&entries);
        })?;
        *self.0.change.borrow_mut() = Some(change);
        Ok(())
    }

    /// 選択の通知。通知の中から設定し直しても二重借用にならないよう、
    /// 呼び出しの間だけクロージャを取り出す。
    fn emit(&self, entries: &[FileEntry]) {
        let Some(mut f) = self.0.on_select.borrow_mut().take() else {
            return;
        };
        f(entries);
        let mut slot = self.0.on_select.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }

    /// `<input>` の `FileList` を [`FileEntry`] の並びへ写す。
    fn read_selection(&self) -> Vec<FileEntry> {
        let Some(files) = self.0.input.files() else {
            return Vec::new();
        };
        if self.0.mode.get().is_folder() {
            // ブラウザはフォルダーの中身を返す。他の環境に合わせて
            // フォルダー 1 つに畳む。
            return files
                .get(0)
                .and_then(|file| folder_name(&file))
                .map(|name| vec![FileEntry::from_name(name)])
                .unwrap_or_default();
        }
        (0..files.length())
            .filter_map(|i| files.get(i))
            .map(|file| FileEntry::from_name(file.name()))
            .collect()
    }

    pub fn set_text(&self, text: &str) {
        self.0.button.set_text_content(Some(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        set_disabled(&self.0.button, !enabled);
        self.0.input.set_disabled(!enabled);
    }

    /// 何を選ばせるかを決める (既定はファイル 1 つ)。
    pub fn set_mode(&self, mode: FilePickerMode) {
        self.0.mode.set(mode);
        self.0.input.set_multiple(mode.allows_multiple());
        self.0.input.set_webkitdirectory(mode.is_folder());
    }

    pub fn mode(&self) -> FilePickerMode {
        self.0.mode.get()
    }

    /// 拡張子で絞り込む。[`FilePickerMode::Folder`] のときは無視される。
    pub fn set_filters(&self, filters: &[FileFilter]) {
        self.0.input.set_accept(&accept_attribute(filters));
    }

    /// 最後に選ばれたもの。まだ選ばれていなければ空。
    pub fn selection(&self) -> Vec<FileEntry> {
        self.0.selection.borrow().clone()
    }

    /// 選ばれたときに呼ばれる。取り消したときは呼ばれない。
    /// 設定し直すと以前のものは外れる。
    pub fn on_select(&self, f: impl FnMut(&[FileEntry]) + 'static) {
        *self.0.on_select.borrow_mut() = Some(Box::new(f));
    }

    /// ダイアログを出す。
    ///
    /// **ブラウザはユーザー操作の中でしかファイル選択を開かない。**
    /// クリックのコールバックなどから呼ぶこと。それ以外の場所から呼んでも
    /// 無視される (エラーにもならない)。
    pub fn open(&self) {
        self.0.input.set_value("");
        self.0.input.click();
    }
}

/// `webkitRelativePath` (`写真/a.png`) の先頭からフォルダー名を取る。
///
/// web-sys は `File::webkitRelativePath` を束ねていないため、
/// プロパティを直接読む。
fn folder_name(file: &web_sys::File) -> Option<String> {
    let value = js_sys::Reflect::get(file, &JsValue::from_str("webkitRelativePath")).ok()?;
    let path = value.as_string()?;
    let head = path.split('/').next()?.to_string();
    if head.is_empty() {
        None
    } else {
        Some(head)
    }
}
