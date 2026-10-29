//! File / folder picking (DOM).
//!
//! In the browser, "letting the user pick" is the `<input type="file">` itself, and
//! the browser (and OS) shows the dialog and draws the list. However, the button
//! text of `<input type="file">` belongs to the browser and cannot be changed, so it
//! is built as **a visible `<button>`, with the `<input>` hidden and the click
//! forwarded to it**.
//!
//! ## Differences from other platforms
//!
//! - **Paths cannot be obtained.** Browsers do not hand out absolute paths, so
//!   [`FileEntry::path`](naui_core::FileEntry::path) is always `None`. Instead,
//!   [`FileEntry::source`](naui_core::FileEntry::source) returns a `blob:` URL
//!   created by `URL.createObjectURL`, so the picked file can be passed straight to
//!   `ui.video()` and the like. **This URL stays valid until the next pick**; picking
//!   again discards the previous ones with `URL.revokeObjectURL`.
//! - **It can only be opened inside a user action.** Calling [`FilePicker::open`]
//!   outside an event such as a click is ignored by the browser.
//! - **When a folder is picked, the browser returns the list of files in it.** The
//!   other platforms return one folder, so the folder name is taken from the start
//!   of `webkitRelativePath` and **only one entry** is returned to match. When you
//!   need the files inside, take the `<input>` from
//!   [`FilePicker::native_element`] and read its `FileList`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{accept_attribute, FileEntry, FileFilter, FilePickerMode, Result};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, Element, HtmlElement, HtmlInputElement};

use crate::to_error;
use crate::widgets::{create, impl_widget, set_disabled, Listener, Widget};

struct FilePickerInner {
    /// A `<span>` holding a `<button>` and a hidden `<input type="file">`.
    element: HtmlElement,
    button: HtmlElement,
    input: HtmlInputElement,
    mode: Cell<FilePickerMode>,
    selection: RefCell<Vec<FileEntry>>,
    on_select: RefCell<Option<Box<dyn FnMut(&[FileEntry])>>>,
    /// The thing that forwards the button's click to the `<input>`.
    click: RefCell<Option<Listener>>,
    change: RefCell<Option<Listener>>,
    /// The `blob:` URLs currently handed out. Revoked on repicking and on drop.
    ///
    /// Without revoking, the picked files' contents would not be freed until the page's
    /// lifetime ends.
    object_urls: RefCell<Vec<String>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

impl Drop for FilePickerInner {
    fn drop(&mut self) {
        revoke_all(&mut self.object_urls.borrow_mut());
    }
}

/// Revokes all the `blob:` URLs handed out.
fn revoke_all(urls: &mut Vec<String>) {
    for url in urls.drain(..) {
        let _ = web_sys::Url::revoke_object_url(&url);
    }
}

/// A button for picking files or folders (`<button>` + `<input type="file">`).
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
            object_urls: RefCell::new(Vec::new()),
            file_drop: Default::default(),
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

    /// The selection notification. So that setting it again from inside the
    /// notification does not double-borrow, the closure is taken out only for the
    /// duration of the call.
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

    /// Maps the `<input>`'s `FileList` into a sequence of [`FileEntry`].
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
        // 以前配った URL はここで取り消す。以降は使えなくなる。
        let mut urls = self.0.object_urls.borrow_mut();
        revoke_all(&mut urls);
        (0..files.length())
            .filter_map(|i| files.get(i))
            .map(|file| {
                let name = file.name();
                // ブラウザがパスを渡さない代わりに、中身を指す URL を作る。
                match web_sys::Url::create_object_url_with_blob(&file) {
                    Ok(url) => {
                        urls.push(url.clone());
                        FileEntry::from_name_and_source(name, url)
                    }
                    Err(_) => FileEntry::from_name(name),
                }
            })
            .collect()
    }

    pub fn set_text(&self, text: &str) {
        self.0.button.set_text_content(Some(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        set_disabled(&self.0.button, !enabled);
        self.0.input.set_disabled(!enabled);
    }

    /// Decides what to let the user pick (one file by default).
    pub fn set_mode(&self, mode: FilePickerMode) {
        self.0.mode.set(mode);
        self.0.input.set_multiple(mode.allows_multiple());
        self.0.input.set_webkitdirectory(mode.is_folder());
    }

    pub fn mode(&self) -> FilePickerMode {
        self.0.mode.get()
    }

    /// Filters by extension. Ignored for [`FilePickerMode::Folder`].
    pub fn set_filters(&self, filters: &[FileFilter]) {
        self.0.input.set_accept(&accept_attribute(filters));
    }

    /// What was picked last. Empty if nothing has been picked yet.
    pub fn selection(&self) -> Vec<FileEntry> {
        self.0.selection.borrow().clone()
    }

    /// Called when something is picked. Not called when the user cancels. Setting it
    /// again removes the previous one.
    pub fn on_select(&self, f: impl FnMut(&[FileEntry]) + 'static) {
        *self.0.on_select.borrow_mut() = Some(Box::new(f));
    }

    /// Shows the dialog.
    ///
    /// **Browsers open file selection only inside a user action.** Call it from a click
    /// callback or similar. Calling it from anywhere else is ignored (without an error,
    /// either).
    pub fn open(&self) {
        self.0.input.set_value("");
        self.0.input.click();
    }
}

/// Takes the folder name from the start of `webkitRelativePath` (`photos/a.png`).
///
/// web-sys does not bind `File::webkitRelativePath`, so the property is read
/// directly.
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
