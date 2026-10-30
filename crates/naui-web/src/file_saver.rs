//! Saving files (DOM).
//!
//! Browsers have no concept of "a destination path", so to match the other
//! platforms the only option is **to hand over the contents and have them written
//! out**. So [`FileSaver`] delivers the bytes from [`FileSaver::set_contents`] in
//! one of two ways:
//!
//! - When **`showSaveFilePicker`** (the File System Access API) is available, the
//!   OS's save dialog appears as is and the contents are written to the chosen
//!   location. This gives the same experience as the three native platforms. Only
//!   Chromium-based browsers have it.
//! - **Otherwise (Firefox / Safari)**, a download via `<a download>`. Where it goes
//!   depends on the browser's settings, and it may land in the downloads folder
//!   without asking.
//!
//! ## Differences from other platforms
//!
//! - **Paths cannot be obtained.** [`FileEntry::path`](naui_core::FileEntry::path)
//!   is always `None`, and only [`FileEntry::name`](naui_core::FileEntry::name) is
//!   set.
//! - **It can only be opened inside a user action.** Calling [`FileSaver::open`]
//!   outside an event such as a click is refused by the browser (delivered to
//!   `on_error`).
//! - **When it becomes a download, completion of the write cannot be awaited.**
//!   `on_save` is called once the browser has taken it over.

use std::cell::RefCell;
use std::rc::Rc;

use js_sys::futures::spawn_local;
use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use naui_core::{with_default_extension, Error, FileEntry, FileFilter, Result};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Blob, Document, Element, HtmlElement};

use crate::to_error;
use crate::widgets::{create, impl_widget, set_disabled, Listener, Widget};

struct FileSaverInner {
    element: HtmlElement,
    document: Document,
    file_name: RefCell<String>,
    filters: RefCell<Vec<FileFilter>>,
    contents: RefCell<Vec<u8>>,
    destination: RefCell<Option<FileEntry>>,
    on_save: RefCell<Option<Box<dyn FnMut(&FileEntry)>>>,
    on_error: RefCell<Option<Box<dyn FnMut(&Error)>>>,
    click: RefCell<Option<Listener>>,
    /// The `blob:` URLs handed out for downloads.
    ///
    /// Revoking them before the download starts would lose the contents, so they are
    /// kept alive until the next save or until the handle is dropped.
    object_urls: RefCell<Vec<String>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

impl Drop for FileSaverInner {
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

/// A button that lets the user write contents to a file (`<button>` +
/// `showSaveFilePicker`).
#[derive(Clone)]
pub struct FileSaver(Rc<FileSaverInner>);
impl_widget!(FileSaver, element);

impl FileSaver {
    pub(crate) fn new(doc: &Document, text: &str) -> Result<Self> {
        let element: HtmlElement = create(doc, "button")?.unchecked_into();
        element.set_text_content(Some(text));

        let this = Self(Rc::new(FileSaverInner {
            element,
            document: doc.clone(),
            file_name: RefCell::new(String::new()),
            filters: RefCell::new(Vec::new()),
            contents: RefCell::new(Vec::new()),
            destination: RefCell::new(None),
            on_save: RefCell::new(None),
            on_error: RefCell::new(None),
            click: RefCell::new(None),
            object_urls: RefCell::new(Vec::new()),
            file_drop: Default::default(),
        }));

        // 保存はユーザー操作の中でしか始められないので、クリックのイベント内で
        // そのままダイアログを開く。
        let weak = Rc::downgrade(&this.0);
        let click = Listener::attach(this.0.element.as_ref(), "click", move || {
            if let Some(inner) = weak.upgrade() {
                FileSaver(inner).open();
            }
        })?;
        *this.0.click.borrow_mut() = Some(click);
        Ok(this)
    }

    pub fn set_text(&self, text: &str) {
        self.0.element.set_text_content(Some(text));
    }

    pub fn set_enabled(&self, enabled: bool) {
        set_disabled(&self.0.element, !enabled);
    }

    /// The name prefilled in the dialog.
    ///
    /// It becomes `suggestedName` for `showSaveFilePicker`, and the `download`
    /// attribute for downloads. If it stays empty and it becomes a download, the
    /// browser's default (`download`) is used.
    pub fn set_file_name(&self, name: &str) {
        *self.0.file_name.borrow_mut() = name.to_string();
    }

    pub fn file_name(&self) -> String {
        self.0.file_name.borrow().clone()
    }

    /// Filters by file type. The first extension becomes the default extension.
    ///
    /// It becomes the `types` of `showSaveFilePicker`. Downloads have no way to choose
    /// a type, so there it is used only to add an extension to the name.
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
    ///
    /// **No path is included** (the browser does not provide one); only the name is.
    pub fn destination(&self) -> Option<FileEntry> {
        self.0.destination.borrow().clone()
    }

    /// Called when writing succeeds. Not called when the user cancels.
    pub fn on_save(&self, f: impl FnMut(&FileEntry) + 'static) {
        *self.0.on_save.borrow_mut() = Some(Box::new(f));
    }

    /// Called when writing fails.
    ///
    /// Calling [`FileSaver::open`] outside a user action also arrives here.
    pub fn on_error(&self, f: impl FnMut(&Error) + 'static) {
        *self.0.on_error.borrow_mut() = Some(Box::new(f));
    }

    /// Shows the dialog. Clicking the button calls the same thing.
    ///
    /// **Browsers can start saving only inside a user action.** Call it from a click
    /// callback or similar.
    pub fn open(&self) {
        if let Err(e) = self.start() {
            emit_error(&self.0, &e);
        }
    }

    /// Turns the contents into a Blob and hands it to whichever mechanism is
    /// available.
    fn start(&self) -> Result<()> {
        let name = with_default_extension(&self.0.file_name.borrow(), &self.0.filters.borrow());
        let blob = blob_from(&self.0.contents.borrow())?;
        let window = web_sys::window()
            .ok_or_else(|| Error::new("show the save dialog", "not running in a browser"))?;

        match save_file_picker(&window) {
            Some(picker) => {
                let filters = self.0.filters.borrow().clone();
                save_through_picker(&self.0, &window, &picker, &name, &filters, blob)
            }
            None => self.download(&name, &blob),
        }
    }

    /// Creates and clicks an `<a download>`. It becomes a browser download.
    fn download(&self, name: &str, blob: &Blob) -> Result<()> {
        // 以前配った URL はここで取り消す。以降は使えなくなる。
        let mut urls = self.0.object_urls.borrow_mut();
        revoke_all(&mut urls);

        let url = web_sys::Url::create_object_url_with_blob(blob)
            .map_err(|e| to_error("create the download URL", e))?;
        urls.push(url.clone());
        drop(urls);

        let anchor: HtmlElement = create(&self.0.document, "a")?.unchecked_into();
        let name = if name.is_empty() { "download" } else { name };
        anchor
            .set_attribute("href", &url)
            .and_then(|_| anchor.set_attribute("download", name))
            .map_err(|e| to_error("build the download", e))?;
        let _ = anchor.style().set_property("display", "none");

        // Firefox は文書に入っていない `<a>` のクリックを無視する。
        let body = self
            .0
            .document
            .body()
            .ok_or_else(|| Error::new("start the download", "there is no body"))?;
        body.append_child(&anchor)
            .map_err(|e| to_error("start the download", e))?;
        anchor.click();
        anchor.remove();

        // ダウンロードは完了を待てないので、引き取られた時点で通知する。
        finish(&self.0, name);
        Ok(())
    }
}

/// Returns `window.showSaveFilePicker` if it exists. `None` otherwise.
fn save_file_picker(window: &web_sys::Window) -> Option<Function> {
    let value = Reflect::get(window, &JsValue::from_str("showSaveFilePicker")).ok()?;
    value.dyn_into::<Function>().ok()
}

/// Writes to the chosen location with the File System Access API.
///
/// It must await `showSaveFilePicker()` → `createWritable()` → `write()` →
/// `close()` in order, so the rest proceeds as a task awaiting Promises. Opening
/// the dialog is done right here (inside the user action).
fn save_through_picker(
    inner: &Rc<FileSaverInner>,
    window: &web_sys::Window,
    picker: &Function,
    name: &str,
    filters: &[FileFilter],
    blob: Blob,
) -> Result<()> {
    let options = Object::new();
    if !name.is_empty() {
        set(&options, "suggestedName", &JsValue::from_str(name))?;
    }
    if let Some(types) = build_types(filters)? {
        set(&options, "types", &types)?;
    }

    let promise: Promise = picker
        .call1(window, &options)
        .map_err(|e| to_error("show the save dialog", e))?
        .dyn_into()
        .map_err(|_| Error::new("show the save dialog", "no Promise was returned"))?;

    let weak = Rc::downgrade(inner);
    spawn_local(async move {
        let result = write_through_handle(promise, blob).await;
        let Some(inner) = weak.upgrade() else {
            return; // ハンドルが捨てられていた。
        };
        match result {
            Ok(Some(name)) => finish(&inner, &name),
            Ok(None) => {} // 取り消された。
            Err(e) => emit_error(&inner, &e),
        }
    });
    Ok(())
}

/// Writes to the chosen file and returns its name. `None` if cancelled.
async fn write_through_handle(promise: Promise, blob: Blob) -> Result<Option<String>> {
    let handle = match promise.await {
        Ok(handle) => handle,
        // 取り消しは失敗として扱わない (他のバックエンドと同じ)。
        Err(reason) if is_abort(&reason) => return Ok(None),
        Err(reason) => return Err(to_error("show the save dialog", reason)),
    };
    let name = Reflect::get(&handle, &JsValue::from_str("name"))
        .ok()
        .and_then(|value| value.as_string())
        .unwrap_or_default();

    let writable = await_promise(call_promise(&handle, "createWritable", None)?).await?;
    await_promise(call_promise(&writable, "write", Some(blob.as_ref()))?).await?;
    // 閉じるまで中身は確定しない。
    await_promise(call_promise(&writable, "close", None)?).await?;
    Ok(Some(name))
}

/// Awaits a Promise and maps failures to naui errors.
async fn await_promise(promise: Promise) -> Result<JsValue> {
    promise.await.map_err(|e| to_error("write the file", e))
}

/// Calls `target.method(arg)` and takes out the returned Promise.
fn call_promise(target: &JsValue, method: &str, arg: Option<&JsValue>) -> Result<Promise> {
    let function: Function = Reflect::get(target, &JsValue::from_str(method))
        .map_err(|e| to_error("write the file", e))?
        .dyn_into()
        .map_err(|_| Error::new("write the file", format!("{method} is not available")))?;
    let returned = match arg {
        Some(arg) => function.call1(target, arg),
        None => function.call0(target),
    }
    .map_err(|e| to_error("write the file", e))?;
    returned.dyn_into().map_err(|_| {
        Error::new(
            "write the file",
            format!("{method} does not return a Promise"),
        )
    })
}

/// Records that the contents were written and notifies.
fn finish(inner: &Rc<FileSaverInner>, name: &str) {
    // ブラウザはパスを渡さないので、名前だけを持つ。
    let entry = FileEntry::from_name(name);
    *inner.destination.borrow_mut() = Some(entry.clone());
    emit_save(inner, &entry);
}

/// So that setting it again from inside the notification does not double-borrow,
/// the closure is taken out only for the duration of the call.
macro_rules! emit_fn {
    ($name:ident, $slot:ident, $arg:ty) => {
        fn $name(inner: &Rc<FileSaverInner>, value: $arg) {
            let Some(mut f) = inner.$slot.borrow_mut().take() else {
                return;
            };
            f(value);
            let mut slot = inner.$slot.borrow_mut();
            if slot.is_none() {
                *slot = Some(f);
            }
        }
    };
}

emit_fn!(emit_save, on_save, &FileEntry);
emit_fn!(emit_error, on_error, &Error);

/// Whether it was a cancel (the dialog was closed).
fn is_abort(reason: &JsValue) -> bool {
    Reflect::get(reason, &JsValue::from_str("name"))
        .ok()
        .and_then(|value| value.as_string())
        .is_some_and(|name| name == "AbortError")
}

/// Turns the contents to write into a `Blob`.
fn blob_from(contents: &[u8]) -> Result<Blob> {
    let parts = Array::of1(&Uint8Array::from(contents));
    Blob::new_with_u8_array_sequence(&parts).map_err(|e| to_error("build the contents to save", e))
}

/// Maps the filters onto the `types` of `showSaveFilePicker`.
fn build_types(filters: &[FileFilter]) -> Result<Option<JsValue>> {
    let usable: Vec<&FileFilter> = filters.iter().filter(|f| !f.is_empty()).collect();
    if usable.is_empty() {
        return Ok(None);
    }
    let types = Array::new();
    for filter in usable {
        let accepted = Array::new();
        for extension in filter.extensions() {
            accepted.push(&JsValue::from_str(&format!(".{extension}")));
        }
        // MIME 型は分からないので、拡張子だけを渡せる汎用の型に載せる。
        let accept = Object::new();
        set(&accept, "application/octet-stream", &accepted)?;

        let entry = Object::new();
        set(&entry, "description", &JsValue::from_str(filter.label()))?;
        set(&entry, "accept", &accept)?;
        types.push(&entry);
    }
    Ok(Some(types.into()))
}

/// Writes a value into a JS object.
fn set(target: &Object, key: &str, value: &JsValue) -> Result<()> {
    Reflect::set(target, &JsValue::from_str(key), value)
        .map(|_| ())
        .map_err(|e| to_error("set up the save dialog", e))
}
