//! ツールバー (`<div role="toolbar">` + `<button>`)。
//!
//! ブラウザにツールバーのコントロールは無いため、WAI-ARIA の
//! `role="toolbar"` を付けた `<div>` にボタンを並べ、区切りは
//! `role="separator"` の `<div>` で表す。見た目はブラウザ既定のままで、
//! CSS は横並びと区切り線の描画にしか使わない。
//!
//! ほかのバックエンドに合わせて [`Widget`](crate::Widget) にはせず、
//! [`Window::set_toolbar`](crate::Window::set_toolbar) でウィンドウの
//! 上端へ取り付ける。
//!
//! **ブラウザには OS のアイコンテーマが無い**ため、アイコンだけは naui が
//! 図形 ([`ToolbarIcon::svg_path`](naui_core::ToolbarIcon::svg_path)) を持ち、
//! インライン SVG として描く。色は `currentColor` に従うので、ブラウザや
//! ページの配色にはそのまま馴染む。`label` は読み上げとツールチップに使う。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{Result, ToolbarIcon, ToolbarItem};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement};

use crate::to_error;
use crate::widgets::{create, set_disabled, Listener};

struct ToolbarInner {
    element: HtmlElement,
    document: Document,
    items: RefCell<Vec<ToolbarItem>>,
    /// 項目と同じ並び。区切りのところは `None`。
    buttons: RefCell<Vec<Option<HtmlElement>>>,
    /// 項目ごとのクリック購読。項目を作り直すと外れる。
    listeners: RefCell<Vec<Listener>>,
    handler: Handler,
    /// ツールバー全体の有効・無効。項目ごとの指定と AND を取る。
    enabled: Cell<bool>,
}

/// 押された項目の通知先。
///
/// 呼び出しの間だけクロージャを取り出すので、通知の中から同じ
/// ツールバーを組み替えても二重借用にならない。
#[derive(Clone, Default)]
struct Handler(Rc<RefCell<Option<Box<dyn FnMut(usize)>>>>);

impl Handler {
    fn set(&self, f: impl FnMut(usize) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, index: usize) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(index);
        let mut slot = self.0.borrow_mut();
        // 呼び出しの中で差し替えられていたら、新しいほうを残す。
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

fn style(element: &HtmlElement, property: &str, value: &str) {
    let _ = element.style().set_property(property, value);
}

/// アイコン 1 つぶんのインライン SVG を作る。
fn icon_svg(doc: &Document, icon: ToolbarIcon) -> Result<Element> {
    const NS: &str = "http://www.w3.org/2000/svg";
    let svg = doc
        .create_element_ns(Some(NS), "svg")
        .map_err(|e| to_error("SVG の生成", e))?;
    let _ = svg.set_attribute("viewBox", "0 0 24 24");
    let _ = svg.set_attribute("width", "16");
    let _ = svg.set_attribute("height", "16");
    // 絵は装飾で、意味はボタンの aria-label が持つ。
    let _ = svg.set_attribute("aria-hidden", "true");
    let _ = svg.set_attribute("fill", "none");
    let _ = svg.set_attribute("stroke", "currentColor");
    let _ = svg.set_attribute("stroke-width", "1.6");
    let _ = svg.set_attribute("stroke-linecap", "round");
    let _ = svg.set_attribute("stroke-linejoin", "round");

    let path = doc
        .create_element_ns(Some(NS), "path")
        .map_err(|e| to_error("SVG パスの生成", e))?;
    let _ = path.set_attribute("d", icon.svg_path());
    append(&svg, &path)?;
    Ok(svg)
}

fn append(parent: &Element, child: &Element) -> Result<()> {
    parent
        .append_child(child)
        .map(|_| ())
        .map_err(|e| to_error("DOM への追加", e))
}

/// ウィンドウの上端に付く、よく使う操作の並び。
///
/// [`Widget`](crate::Widget) ではない。
/// [`Window::set_toolbar`](crate::Window::set_toolbar) で取り付ける。
/// ナビゲーションと違い**選ばれている項目を持たず**、押されるたびに
/// そのインデックスで [`on_activate`](Self::on_activate) が呼ばれる。
/// インデックスは区切りを含めた並びの位置で、区切りが返ることはない。
#[derive(Clone)]
pub struct Toolbar(Rc<ToolbarInner>);

impl Toolbar {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let element: HtmlElement = create(doc, "div")?.unchecked_into();
        let _ = element.set_attribute("role", "toolbar");
        style(&element, "display", "flex");
        style(&element, "flex-direction", "row");
        style(&element, "align-items", "center");
        style(&element, "gap", "6px");
        // ウィンドウの上端に置くので、縦には縮まない。
        style(&element, "flex-shrink", "0");
        Ok(Self(Rc::new(ToolbarInner {
            element,
            document: doc.clone(),
            items: RefCell::new(Vec::new()),
            buttons: RefCell::new(Vec::new()),
            listeners: RefCell::new(Vec::new()),
            handler: Handler::default(),
            enabled: Cell::new(true),
        })))
    }

    /// 項目を作り直す。以前の項目は取り除かれる。
    ///
    /// インデックスは区切りを含めた並びの位置。
    pub fn set_items(&self, items: &[ToolbarItem]) {
        let _ = self.rebuild(items);
    }

    fn rebuild(&self, items: &[ToolbarItem]) -> Result<()> {
        let doc = self.0.document.clone();
        self.0.element.set_inner_html("");
        self.0.buttons.borrow_mut().clear();
        self.0.listeners.borrow_mut().clear();

        let whole = self.0.enabled.get();
        let mut buttons = Vec::with_capacity(items.len());
        let mut listeners = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            if item.is_separator() {
                let separator: HtmlElement = create(&doc, "div")?.unchecked_into();
                let _ = separator.set_attribute("role", "separator");
                let _ = separator.set_attribute("aria-orientation", "vertical");
                style(&separator, "width", "1px");
                style(&separator, "align-self", "stretch");
                style(&separator, "background", "currentColor");
                style(&separator, "opacity", "0.3");
                append(&self.0.element, &separator)?;
                buttons.push(None);
                continue;
            }

            let button: HtmlElement = create(&doc, "button")?.unchecked_into();
            let _ = button.set_attribute("type", "button");
            // 絵だけでは何のボタンか伝わらないので、読み上げ名と
            // ツールチップにラベルを入れる。
            let _ = button.set_attribute("aria-label", &item.label);
            let _ = button.set_attribute("title", &item.label);
            style(&button, "display", "inline-flex");
            style(&button, "align-items", "center");
            style(&button, "justify-content", "center");
            append(&button, &icon_svg(&doc, item.icon)?)?;
            set_disabled(&button, !(item.enabled && whole));

            // ハンドルを強く持つと購読との間で循環するため、弱参照にする。
            let listener = Listener::attach(button.as_ref(), "click", {
                let weak = Rc::downgrade(&self.0);
                move || {
                    if let Some(inner) = weak.upgrade() {
                        inner.handler.emit(index);
                    }
                }
            })?;
            listeners.push(listener);
            append(&self.0.element, &button)?;
            buttons.push(Some(button));
        }

        *self.0.buttons.borrow_mut() = buttons;
        *self.0.listeners.borrow_mut() = listeners;
        self.0.items.borrow_mut().clear();
        self.0.items.borrow_mut().extend_from_slice(items);
        Ok(())
    }

    /// 区切りを含めた項目数。
    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 項目 1 つの有効・無効を変える。区切りと範囲外は何もしない。
    pub fn set_item_enabled(&self, index: usize, enabled: bool) {
        let mut items = self.0.items.borrow_mut();
        let Some(item) = items.get_mut(index) else {
            return;
        };
        if item.is_separator() {
            return;
        }
        item.enabled = enabled;
        drop(items);
        self.apply_enabled();
    }

    /// いま押せる項目か。区切りと範囲外は `false`。
    pub fn is_item_enabled(&self, index: usize) -> bool {
        self.0.enabled.get()
            && self
                .0
                .items
                .borrow()
                .get(index)
                .is_some_and(|item| !item.is_separator() && item.enabled)
    }

    /// ツールバー全体の有効・無効を変える。項目ごとの指定は残る。
    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        self.apply_enabled();
    }

    /// 項目ごとの指定と全体の指定を DOM へ反映する。
    fn apply_enabled(&self) {
        let whole = self.0.enabled.get();
        let items = self.0.items.borrow();
        for (button, item) in self.0.buttons.borrow().iter().zip(items.iter()) {
            if let Some(button) = button {
                set_disabled(button, !(item.enabled && whole));
            }
        }
    }

    /// 利用者が押したのと同じように項目を実行する。
    ///
    /// 区切り・押せない項目・範囲外は何もしない。
    pub fn activate(&self, index: usize) {
        if self.is_item_enabled(index) {
            self.0.handler.emit(index);
        }
    }

    /// 項目が押されたときに、そのインデックスで呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_activate(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// 項目に対応する `<button>`。区切りと範囲外は `None`。
    /// バックエンド固有の脱出口として公開している。
    pub fn native_button(&self, index: usize) -> Option<HtmlElement> {
        self.0.buttons.borrow().get(index)?.clone()
    }

    /// ツールバーの `<div role="toolbar">`。
    /// バックエンド固有の脱出口として公開している。
    pub fn native_element(&self) -> Element {
        self.0.element.clone().unchecked_into()
    }

    /// ウィンドウへ差し込む要素。[`crate::Window`] だけが使う。
    pub(crate) fn mount(&self) -> HtmlElement {
        self.0.element.clone()
    }
}
