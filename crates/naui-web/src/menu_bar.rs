//! メニューバー (`<div role="menubar">` + `<div role="menu">`)。
//!
//! ブラウザには OS のアプリケーションメニューが無く、`<menu>` 要素も
//! ただのリストなので、ここだけは **WAI-ARIA の役割を付けた要素で合成**する。
//!
//! | naui | DOM |
//! | --- | --- |
//! | `MenuBar` | `<div role="menubar">` |
//! | 見出し | `<button role="menuitem" aria-haspopup="true">` |
//! | メニュー | `<div role="menu">` (見出しの下・`position: absolute`) |
//! | 項目 | `<button role="menuitem">` |
//! | 区切り線 | `<div role="separator">` |
//!
//! 色は CSS のシステムカラー (`Canvas` / `CanvasText`) を使うので、
//! [`crate::Ui::set_theme`] が `<html>` へ設定する `color-scheme` に
//! そのまま追従する ([`crate::PopupMenu`] と同じ)。
//!
//! ショートカットもブラウザには無い概念なので、`document` の `keydown` を
//! 見張って naui が突き合わせる。主修飾キーは Ctrl と ⌘ のどちらでもよい
//! (macOS のブラウザでは ⌘ が主修飾キーになるため)。項目の右端には
//! [`MenuShortcut::label`](naui_core::MenuShortcut::label) を出す。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{MenuSpec, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement, KeyboardEvent, Node};

use crate::to_error;
use crate::widgets::{create, set_disabled, Listener};

fn style(element: &HtmlElement, property: &str, value: &str) {
    let _ = element.style().set_property(property, value);
}

/// 押された項目の通知先。
///
/// 呼び出しの間だけクロージャを取り出すので、通知の中から同じメニューバーを
/// 組み替えても二重借用にならない。
#[derive(Clone, Default)]
struct Handler(Rc<RefCell<Option<Box<dyn FnMut(usize, usize)>>>>);

impl Handler {
    fn set(&self, f: impl FnMut(usize, usize) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, menu: usize, item: usize) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(menu, item);
        let mut slot = self.0.borrow_mut();
        // 呼び出しの中で差し替えられていたら、新しいほうを残す。
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// 見出し 1 つぶんの DOM。
struct MenuParts {
    /// 見出しのボタン。
    title: HtmlElement,
    /// 見出しの下に出るメニュー本体。
    popup: HtmlElement,
    /// 項目ごとのボタン。区切り線の位置は `None`。
    items: Vec<Option<HtmlElement>>,
}

struct MenuBarInner {
    element: HtmlElement,
    document: Document,
    menus: RefCell<Vec<MenuSpec>>,
    parts: RefCell<Vec<MenuParts>>,
    /// 見出しと項目のクリック購読。メニューを作り直すと外れる。
    listeners: RefCell<Vec<Listener>>,
    /// 外側を押したとき・Escape・ショートカットのための購読。
    _document_listeners: RefCell<Vec<Listener>>,
    handler: Handler,
    /// いま開いている見出し。
    open: Cell<Option<usize>>,
    /// メニューバー全体の有効・無効。項目ごとの指定と AND を取る。
    enabled: Cell<bool>,
}

fn append(parent: &Element, child: &Element) -> Result<()> {
    parent
        .append_child(child)
        .map(|_| ())
        .map_err(|e| to_error("DOM への追加", e))
}

/// ウィンドウの上端に付く、OS のアプリケーションメニュー相当。
///
/// [`Widget`](crate::Widget) ではない。
/// [`Window::set_menu_bar`](crate::Window::set_menu_bar) で取り付ける。
/// 項目が押されるたびに、その **(見出しのインデックス, 項目のインデックス)**
/// で [`on_activate`](Self::on_activate) が呼ばれる。項目のインデックスは
/// 区切り線を含めた並びの位置で、区切り線が返ることはない。
#[derive(Clone)]
pub struct MenuBar(Rc<MenuBarInner>);

impl MenuBar {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let element: HtmlElement = create(doc, "div")?.unchecked_into();
        let _ = element.set_attribute("role", "menubar");
        style(&element, "display", "flex");
        style(&element, "flex-direction", "row");
        style(&element, "align-items", "stretch");
        style(&element, "gap", "2px");
        // ウィンドウの上端に置くので、縦には縮まない。
        style(&element, "flex-shrink", "0");

        let this = Self(Rc::new(MenuBarInner {
            element,
            document: doc.clone(),
            menus: RefCell::new(Vec::new()),
            parts: RefCell::new(Vec::new()),
            listeners: RefCell::new(Vec::new()),
            _document_listeners: RefCell::new(Vec::new()),
            handler: Handler::default(),
            open: Cell::new(None),
            enabled: Cell::new(true),
        }));
        this.install_document_listeners(doc)?;
        Ok(this)
    }

    /// 外側を押したとき・Escape で閉じ、ショートカットを拾うようにする。
    fn install_document_listeners(&self, doc: &Document) -> Result<()> {
        let target: web_sys::EventTarget = doc.clone().unchecked_into();

        let outside = Listener::attach_event(&target, "pointerdown", {
            let weak = Rc::downgrade(&self.0);
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                if inner.open.get().is_none() {
                    return;
                }
                let node = event.target().and_then(|t| t.dyn_into::<Node>().ok());
                if !inner.element.contains(node.as_ref()) {
                    MenuBar(inner).close();
                }
            }
        })?;

        let keys = Listener::attach_event(&target, "keydown", {
            let weak = Rc::downgrade(&self.0);
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let Some(key) = event.dyn_ref::<KeyboardEvent>() else {
                    return;
                };
                // 変換中のキーは IME のものなので触らない。
                if key.is_composing() {
                    return;
                }
                let bar = MenuBar(inner);
                if key.key() == "Escape" {
                    if bar.0.open.get().is_some() {
                        // Safari は全画面のとき、ページが既定動作を止めない
                        // 限り Esc を全画面の解除に使ってしまう。
                        event.prevent_default();
                        bar.close();
                    }
                    return;
                }
                // 主修飾キーは Ctrl と ⌘ のどちらでもよい。
                let primary = key.ctrl_key() || key.meta_key();
                if !primary {
                    return;
                }
                if let Some((menu, item)) =
                    bar.find_shortcut(&key.key(), primary, key.shift_key(), key.alt_key())
                {
                    // ブラウザの既定のショートカット (⌘S など) は起こさない。
                    event.prevent_default();
                    bar.close();
                    bar.0.handler.emit(menu, item);
                }
            }
        })?;

        *self.0._document_listeners.borrow_mut() = vec![outside, keys];
        Ok(())
    }

    /// 押されたキーに合う、いま押せる項目を探す。
    ///
    /// 借用を返す前に結果を取り出しておく (通知の中でメニューを
    /// 組み替えられても二重借用にならないようにするため)。
    fn find_shortcut(
        &self,
        key: &str,
        primary: bool,
        shift: bool,
        alt: bool,
    ) -> Option<(usize, usize)> {
        if !self.0.enabled.get() {
            return None;
        }
        let menus = self.0.menus.borrow();
        for (menu, spec) in menus.iter().enumerate() {
            for (item, entry) in spec.items.iter().enumerate() {
                if entry.is_separator() || !entry.enabled {
                    continue;
                }
                if entry
                    .shortcut
                    .is_some_and(|s| s.matches(key, primary, shift, alt))
                {
                    return Some((menu, item));
                }
            }
        }
        None
    }

    /// メニューを作り直す。以前のメニューは取り除かれる。
    ///
    /// 見出しのインデックスは渡した並びの位置、項目のインデックスは
    /// 区切り線を含めた並びの位置。
    pub fn set_menus(&self, menus: &[MenuSpec]) {
        let _ = self.rebuild(menus);
    }

    fn rebuild(&self, menus: &[MenuSpec]) -> Result<()> {
        let doc = self.0.document.clone();
        self.0.element.set_inner_html("");
        self.0.parts.borrow_mut().clear();
        self.0.listeners.borrow_mut().clear();
        self.0.open.set(None);

        let whole = self.0.enabled.get();
        let mut parts = Vec::with_capacity(menus.len());
        let mut listeners = Vec::new();
        for (menu_index, spec) in menus.iter().enumerate() {
            // 見出しとメニュー本体を、位置決めの基準になる入れ物へ入れる。
            let holder: HtmlElement = create(&doc, "div")?.unchecked_into();
            style(&holder, "position", "relative");
            style(&holder, "display", "flex");

            let title: HtmlElement = create(&doc, "button")?.unchecked_into();
            let _ = title.set_attribute("type", "button");
            let _ = title.set_attribute("role", "menuitem");
            let _ = title.set_attribute("aria-haspopup", "true");
            let _ = title.set_attribute("aria-expanded", "false");
            title.set_text_content(Some(&spec.title));
            set_disabled(&title, !whole);
            listeners.push(Listener::attach(title.as_ref(), "click", {
                let weak = Rc::downgrade(&self.0);
                move || {
                    if let Some(inner) = weak.upgrade() {
                        MenuBar(inner).toggle(menu_index);
                    }
                }
            })?);
            append(&holder, &title)?;

            let popup: HtmlElement = create(&doc, "div")?.unchecked_into();
            let _ = popup.set_attribute("role", "menu");
            let _ = popup.set_attribute("aria-label", &spec.title);
            // 位置決めと重なりだけを CSS で作る。色はシステムカラーに任せる。
            style(&popup, "position", "absolute");
            style(&popup, "top", "100%");
            style(&popup, "left", "0");
            style(&popup, "display", "none");
            style(&popup, "z-index", "1000");
            style(&popup, "min-width", "12em");
            style(&popup, "padding", "4px");
            style(&popup, "background", "Canvas");
            style(&popup, "color", "CanvasText");
            style(&popup, "border", "1px solid CanvasText");
            style(&popup, "border-radius", "6px");
            style(&popup, "box-shadow", "0 4px 12px rgba(0, 0, 0, 0.25)");

            let mut items = Vec::with_capacity(spec.items.len());
            for (item_index, entry) in spec.items.iter().enumerate() {
                if entry.is_separator() {
                    let separator: HtmlElement = create(&doc, "div")?.unchecked_into();
                    let _ = separator.set_attribute("role", "separator");
                    style(&separator, "height", "1px");
                    style(&separator, "margin", "4px 0");
                    style(&separator, "background", "currentColor");
                    style(&separator, "opacity", "0.3");
                    append(&popup, &separator)?;
                    items.push(None);
                    continue;
                }

                let button: HtmlElement = create(&doc, "button")?.unchecked_into();
                let _ = button.set_attribute("type", "button");
                let _ = button.set_attribute("role", "menuitem");
                style(&button, "display", "flex");
                style(&button, "width", "100%");
                style(&button, "align-items", "center");
                style(&button, "justify-content", "space-between");
                style(&button, "gap", "2em");
                style(&button, "text-align", "left");

                let label: HtmlElement = create(&doc, "span")?.unchecked_into();
                label.set_text_content(Some(&entry.label));
                append(&button, &label)?;
                if let Some(shortcut) = entry.shortcut {
                    // ブラウザはショートカットの表示を持たないので、naui が添える。
                    let hint: HtmlElement = create(&doc, "span")?.unchecked_into();
                    hint.set_text_content(Some(&shortcut.label()));
                    // 読み上げでは操作名のあとに続けて読ませる。
                    let _ = hint.set_attribute("aria-hidden", "true");
                    style(&hint, "opacity", "0.6");
                    append(&button, &hint)?;
                    let _ = button.set_attribute(
                        "aria-keyshortcuts",
                        &aria_key_shortcuts(shortcut.shift, shortcut.alt, shortcut.key),
                    );
                }
                set_disabled(&button, !(entry.enabled && whole));

                listeners.push(Listener::attach(button.as_ref(), "click", {
                    let weak = Rc::downgrade(&self.0);
                    move || {
                        let Some(inner) = weak.upgrade() else {
                            return;
                        };
                        let bar = MenuBar(inner);
                        bar.close();
                        bar.0.handler.emit(menu_index, item_index);
                    }
                })?);
                append(&popup, &button)?;
                items.push(Some(button));
            }

            append(&holder, &popup)?;
            append(&self.0.element, &holder)?;
            parts.push(MenuParts {
                title,
                popup,
                items,
            });
        }

        *self.0.parts.borrow_mut() = parts;
        *self.0.listeners.borrow_mut() = listeners;
        let mut stored = self.0.menus.borrow_mut();
        stored.clear();
        stored.extend_from_slice(menus);
        Ok(())
    }

    /// 見出しの数。
    pub fn len(&self) -> usize {
        self.0.menus.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 見出し 1 つが持つ、区切り線を含めた項目数。範囲外は `0`。
    pub fn menu_len(&self, menu: usize) -> usize {
        self.0
            .menus
            .borrow()
            .get(menu)
            .map_or(0, |spec| spec.items.len())
    }

    /// 項目 1 つの有効・無効を変える。区切り線と範囲外は何もしない。
    pub fn set_item_enabled(&self, menu: usize, item: usize, enabled: bool) {
        let mut menus = self.0.menus.borrow_mut();
        let Some(entry) = menus
            .get_mut(menu)
            .and_then(|spec| spec.items.get_mut(item))
        else {
            return;
        };
        if entry.is_separator() {
            return;
        }
        entry.enabled = enabled;
        drop(menus);
        self.apply_enabled();
    }

    /// いま押せる項目か。区切り線と範囲外は `false`。
    pub fn is_item_enabled(&self, menu: usize, item: usize) -> bool {
        self.0.enabled.get()
            && self
                .0
                .menus
                .borrow()
                .get(menu)
                .and_then(|spec| spec.items.get(item))
                .is_some_and(|entry| !entry.is_separator() && entry.enabled)
    }

    /// メニューバー全体の有効・無効を変える。項目ごとの指定は残る。
    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        if !enabled {
            self.close();
        }
        self.apply_enabled();
    }

    /// 項目ごとの指定と全体の指定を DOM へ反映する。
    fn apply_enabled(&self) {
        let whole = self.0.enabled.get();
        let menus = self.0.menus.borrow();
        for (spec, parts) in menus.iter().zip(self.0.parts.borrow().iter()) {
            set_disabled(&parts.title, !whole);
            for (entry, button) in spec.items.iter().zip(parts.items.iter()) {
                if let Some(button) = button {
                    set_disabled(button, !(entry.enabled && whole));
                }
            }
        }
    }

    /// 見出しを押したときの開閉。
    fn toggle(&self, menu: usize) {
        if self.0.open.get() == Some(menu) {
            self.close();
        } else {
            self.open(menu);
        }
    }

    /// 見出し 1 つのメニューを出す。範囲外と無効なときは何もしない。
    ///
    /// ほかの 3 環境には「メニューをプログラムから開く」手段が無いので、
    /// 公開 API にはせず、見出しを押したときだけ使う。
    fn open(&self, menu: usize) {
        if !self.0.enabled.get() || menu >= self.len() {
            return;
        }
        self.close();
        let parts = self.0.parts.borrow();
        let Some(parts) = parts.get(menu) else {
            return;
        };
        style(&parts.popup, "display", "block");
        let _ = parts.title.set_attribute("aria-expanded", "true");
        self.0.open.set(Some(menu));
    }

    /// 開いているメニューを閉じる。開いていなければ何もしない。
    pub(crate) fn close(&self) {
        let Some(menu) = self.0.open.take() else {
            return;
        };
        let parts = self.0.parts.borrow();
        if let Some(parts) = parts.get(menu) {
            style(&parts.popup, "display", "none");
            let _ = parts.title.set_attribute("aria-expanded", "false");
        }
    }

    /// 利用者が選んだのと同じように項目を実行する。
    ///
    /// 区切り線・押せない項目・範囲外は何もしない。
    pub fn activate(&self, menu: usize, item: usize) {
        if self.is_item_enabled(menu, item) {
            self.0.handler.emit(menu, item);
        }
    }

    /// 項目が押されたときに、その見出しと項目のインデックスで呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_activate(&self, f: impl FnMut(usize, usize) + 'static) {
        self.0.handler.set(f);
    }

    /// メニューバーの `<div role="menubar">`。
    /// バックエンド固有の脱出口として公開している。
    pub fn native_element(&self) -> Element {
        self.0.element.clone().unchecked_into()
    }

    /// 見出しの `<button>`。範囲外は `None`。
    /// バックエンド固有の脱出口として公開している。
    pub fn native_title(&self, menu: usize) -> Option<HtmlElement> {
        Some(self.0.parts.borrow().get(menu)?.title.clone())
    }

    /// 項目の `<button>`。区切り線と範囲外は `None`。
    /// バックエンド固有の脱出口として公開している。
    pub fn native_item(&self, menu: usize, item: usize) -> Option<HtmlElement> {
        self.0.parts.borrow().get(menu)?.items.get(item)?.clone()
    }

    /// 見出しの下に出る `<div role="menu">`。範囲外は `None`。
    /// バックエンド固有の脱出口として公開している。
    pub fn native_menu(&self, menu: usize) -> Option<HtmlElement> {
        Some(self.0.parts.borrow().get(menu)?.popup.clone())
    }

    /// ウィンドウへ差し込む要素。[`crate::Window`] だけが使う。
    pub(crate) fn mount(&self) -> HtmlElement {
        self.0.element.clone()
    }
}

/// `aria-keyshortcuts` の書き方 (`Control+Shift+S`)。
///
/// WAI-ARIA は修飾キーの名前を DOM の `KeyboardEvent.key` にそろえるよう
/// 求めるので、表示用の [`MenuShortcut::label`](naui_core::MenuShortcut::label)
/// とは別に組み立てる。
fn aria_key_shortcuts(shift: bool, alt: bool, key: char) -> String {
    let mut out = String::from("Control+");
    if shift {
        out.push_str("Shift+");
    }
    if alt {
        out.push_str("Alt+");
    }
    out.push(key.to_ascii_uppercase());
    out
}
