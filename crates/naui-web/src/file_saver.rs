//! ファイルの保存 (DOM)。
//!
//! ブラウザには「保存先のパス」という概念が無いため、他の環境と同じ形にする
//! には**内容を渡して書き出させる**しかない。そこで [`FileSaver`] は
//! [`FileSaver::set_contents`] のバイト列を、次の 2 つのどちらかで届ける。
//!
//! - **`showSaveFilePicker`** (File System Access API) があるとき。
//!   OS の保存ダイアログがそのまま出て、選ばれた場所へ書き込む。
//!   ネイティブの 3 環境と同じ体験になる。Chromium 系のみが持つ。
//! - **無いとき (Firefox / Safari)** は `<a download>` のダウンロード。
//!   保存先はブラウザの設定次第で、確認なしにダウンロードフォルダーへ
//!   落ちることもある。
//!
//! ## 他の環境との違い
//!
//! - **パスは取れない。** [`FileEntry::path`](naui_core::FileEntry::path) は
//!   常に `None` で、[`FileEntry::name`](naui_core::FileEntry::name) だけが入る。
//! - **ユーザー操作の中でしか開けない。** [`FileSaver::open`] をクリック等の
//!   イベント外から呼ぶと、ブラウザに拒否される (`on_error` へ届く)。
//! - **ダウンロードになったときは、書き込みの完了を待てない。**
//!   ブラウザが引き取った時点で `on_save` を呼ぶ。

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
    /// ダウンロードのために配った `blob:` URL。
    ///
    /// ダウンロードが始まる前に取り消すと中身が失われるため、次の保存か
    /// ハンドルの破棄まで生かしておく。
    object_urls: RefCell<Vec<String>>,
}

impl Drop for FileSaverInner {
    fn drop(&mut self) {
        revoke_all(&mut self.object_urls.borrow_mut());
    }
}

/// 配ってあった `blob:` URL をまとめて取り消す。
fn revoke_all(urls: &mut Vec<String>) {
    for url in urls.drain(..) {
        let _ = web_sys::Url::revoke_object_url(&url);
    }
}

/// 内容をファイルへ書き出させるボタン (`<button>` + `showSaveFilePicker`)。
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

    /// ダイアログに最初から入れておく名前。
    ///
    /// `showSaveFilePicker` では `suggestedName`、ダウンロードでは
    /// `download` 属性になる。空のままダウンロードになった場合は
    /// ブラウザの既定 (`download`) が使われる。
    pub fn set_file_name(&self, name: &str) {
        *self.0.file_name.borrow_mut() = name.to_string();
    }

    pub fn file_name(&self) -> String {
        self.0.file_name.borrow().clone()
    }

    /// 種類の絞り込み。先頭の拡張子が既定の拡張子になる。
    ///
    /// `showSaveFilePicker` の `types` になる。ダウンロードでは種類を選ばせる
    /// 仕組みが無いため、名前へ拡張子を補うためだけに使われる。
    pub fn set_filters(&self, filters: &[FileFilter]) {
        *self.0.filters.borrow_mut() = filters.to_vec();
    }

    /// 書き出す内容。保存のたびに、このバイト列がそのまま書かれる。
    pub fn set_contents(&self, contents: &[u8]) {
        *self.0.contents.borrow_mut() = contents.to_vec();
    }

    /// 書き出す内容の大きさ (バイト数)。
    pub fn contents_len(&self) -> usize {
        self.0.contents.borrow().len()
    }

    /// 最後に書き出した先。まだ保存していなければ `None`。
    ///
    /// **パスは入らない** (ブラウザが渡さないため)。名前だけが入る。
    pub fn destination(&self) -> Option<FileEntry> {
        self.0.destination.borrow().clone()
    }

    /// 書き出しに成功したときに呼ばれる。取り消したときは呼ばれない。
    pub fn on_save(&self, f: impl FnMut(&FileEntry) + 'static) {
        *self.0.on_save.borrow_mut() = Some(Box::new(f));
    }

    /// 書き出しに失敗したときに呼ばれる。
    ///
    /// ユーザー操作の外から [`FileSaver::open`] を呼んだときもここへ届く。
    pub fn on_error(&self, f: impl FnMut(&Error) + 'static) {
        *self.0.on_error.borrow_mut() = Some(Box::new(f));
    }

    /// ダイアログを出す。ボタンを押したときにも同じものが呼ばれる。
    ///
    /// **ブラウザはユーザー操作の中でしか保存を始められない。**
    /// クリックのコールバックなどから呼ぶこと。
    pub fn open(&self) {
        if let Err(e) = self.start() {
            emit_error(&self.0, &e);
        }
    }

    /// 内容を Blob にして、使える方の仕組みへ渡す。
    fn start(&self) -> Result<()> {
        let name = with_default_extension(&self.0.file_name.borrow(), &self.0.filters.borrow());
        let blob = blob_from(&self.0.contents.borrow())?;
        let window = web_sys::window()
            .ok_or_else(|| Error::new("保存ダイアログ", "ブラウザ環境ではありません"))?;

        match save_file_picker(&window) {
            Some(picker) => {
                let filters = self.0.filters.borrow().clone();
                save_through_picker(&self.0, &window, &picker, &name, &filters, blob)
            }
            None => self.download(&name, &blob),
        }
    }

    /// `<a download>` を作って押す。ブラウザのダウンロードになる。
    fn download(&self, name: &str, blob: &Blob) -> Result<()> {
        // 以前配った URL はここで取り消す。以降は使えなくなる。
        let mut urls = self.0.object_urls.borrow_mut();
        revoke_all(&mut urls);

        let url = web_sys::Url::create_object_url_with_blob(blob)
            .map_err(|e| to_error("ダウンロード URL の生成", e))?;
        urls.push(url.clone());
        drop(urls);

        let anchor: HtmlElement = create(&self.0.document, "a")?.unchecked_into();
        let name = if name.is_empty() { "download" } else { name };
        anchor
            .set_attribute("href", &url)
            .and_then(|_| anchor.set_attribute("download", name))
            .map_err(|e| to_error("ダウンロードの組み立て", e))?;
        let _ = anchor.style().set_property("display", "none");

        // Firefox は文書に入っていない `<a>` のクリックを無視する。
        let body = self
            .0
            .document
            .body()
            .ok_or_else(|| Error::new("ダウンロードの実行", "body がありません"))?;
        body.append_child(&anchor)
            .map_err(|e| to_error("ダウンロードの実行", e))?;
        anchor.click();
        anchor.remove();

        // ダウンロードは完了を待てないので、引き取られた時点で通知する。
        finish(&self.0, name);
        Ok(())
    }
}

/// `window.showSaveFilePicker` があれば返す。無ければ `None`。
fn save_file_picker(window: &web_sys::Window) -> Option<Function> {
    let value = Reflect::get(window, &JsValue::from_str("showSaveFilePicker")).ok()?;
    value.dyn_into::<Function>().ok()
}

/// File System Access API で、選ばれた場所へ書き込む。
///
/// `showSaveFilePicker()` → `createWritable()` → `write()` → `close()` の順に
/// 待つ必要があるので、続きは Promise を待つタスクとして進める。
/// ダイアログを開くところまでは、この場 (ユーザー操作の中) で行う。
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
        .map_err(|e| to_error("保存ダイアログの表示", e))?
        .dyn_into()
        .map_err(|_| Error::new("保存ダイアログの表示", "Promise が返りませんでした"))?;

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

/// 選ばれたファイルへ書き込み、その名前を返す。取り消しは `None`。
async fn write_through_handle(promise: Promise, blob: Blob) -> Result<Option<String>> {
    let handle = match promise.await {
        Ok(handle) => handle,
        // 取り消しは失敗として扱わない (他のバックエンドと同じ)。
        Err(reason) if is_abort(&reason) => return Ok(None),
        Err(reason) => return Err(to_error("保存ダイアログの表示", reason)),
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

/// Promise を待って、失敗を naui のエラーへ写す。
async fn await_promise(promise: Promise) -> Result<JsValue> {
    promise.await.map_err(|e| to_error("ファイルの書き出し", e))
}

/// `target.method(arg)` を呼んで、返る Promise を取り出す。
fn call_promise(target: &JsValue, method: &str, arg: Option<&JsValue>) -> Result<Promise> {
    let function: Function = Reflect::get(target, &JsValue::from_str(method))
        .map_err(|e| to_error("ファイルの書き出し", e))?
        .dyn_into()
        .map_err(|_| Error::new("ファイルの書き出し", format!("{method} がありません")))?;
    let returned = match arg {
        Some(arg) => function.call1(target, arg),
        None => function.call0(target),
    }
    .map_err(|e| to_error("ファイルの書き出し", e))?;
    returned.dyn_into().map_err(|_| {
        Error::new(
            "ファイルの書き出し",
            format!("{method} が Promise を返しません"),
        )
    })
}

/// 書き出せたことを記録して通知する。
fn finish(inner: &Rc<FileSaverInner>, name: &str) {
    // ブラウザはパスを渡さないので、名前だけを持つ。
    let entry = FileEntry::from_name(name);
    *inner.destination.borrow_mut() = Some(entry.clone());
    emit_save(inner, &entry);
}

/// 通知の中から設定し直しても二重借用にならないよう、
/// 呼び出しの間だけクロージャを取り出す。
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

/// 取り消し (ダイアログを閉じた) かどうか。
fn is_abort(reason: &JsValue) -> bool {
    Reflect::get(reason, &JsValue::from_str("name"))
        .ok()
        .and_then(|value| value.as_string())
        .is_some_and(|name| name == "AbortError")
}

/// 書き出す内容を `Blob` にする。
fn blob_from(contents: &[u8]) -> Result<Blob> {
    let parts = Array::of1(&Uint8Array::from(contents));
    Blob::new_with_u8_array_sequence(&parts).map_err(|e| to_error("保存する内容の組み立て", e))
}

/// 絞り込みを `showSaveFilePicker` の `types` へ写す。
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

/// JS のオブジェクトへ値を書く。
fn set(target: &Object, key: &str, value: &JsValue) -> Result<()> {
    Reflect::set(target, &JsValue::from_str(key), value)
        .map(|_| ())
        .map_err(|e| to_error("保存ダイアログの設定", e))
}
