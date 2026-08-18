//! ポップアップ (コンテキスト) メニュー (DOM)。
//!
//! ブラウザにはメニューを出す標準 API が無い (`<menu>` 要素はただの
//! リストで、ブラウザ既定のコンテキストメニューは JavaScript から
//! 差し替えられない)。そのため、ここだけは
//! **`<div role="menu">` と `<button role="menuitem">` で合成**する。
//!
//! 色は CSS のシステムカラー (`Canvas` / `CanvasText`) を使うので、
//! [`crate::Ui::set_theme`] が `<html>` に設定する `color-scheme` に
//! そのまま追従する。naui が自前で配色を決めることはない。
//!
//! | naui | DOM |
//! | --- | --- |
//! | `PopupMenu` | `<div role="menu">` (`<body>` 直下・`position: fixed`) |
//! | 項目 | `<button role="menuitem">` |
//! | 区切り線 | `<div role="separator">` |

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{Error, PopupItem, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement, KeyboardEvent, MouseEvent, Node};

use crate::navigation::SelectHandler;
use crate::to_error;
use crate::widgets::{create, set_disabled, Listener, Widget};

fn style(element: &HtmlElement, property: &str, value: &str) {
    let _ = element.style().set_property(property, value);
}

struct PopupMenuInner {
    document: Document,
    /// メニュー本体。`<body>` 直下に置き、出ていない間は `display: none`。
    element: HtmlElement,
    /// 項目ごとのボタン。区切り線の位置は `None`。
    buttons: RefCell<Vec<Option<HtmlElement>>>,
    /// 項目のクリック購読。項目を作り直すと外れる。
    item_listeners: RefCell<Vec<Listener>>,
    /// 取り付けたウィジェットの `contextmenu` 購読とハンドル。
    attached: RefCell<Vec<(Listener, Box<dyn Widget>)>>,
    /// メニューの外側を押したときと Escape で閉じるための購読。
    _dismiss: RefCell<Vec<Listener>>,
    handler: SelectHandler,
    open: Cell<bool>,
}

/// ポップアップ (コンテキスト) メニュー。
///
/// 画面に並ぶウィジェットではないので [`Widget`] ではない。
/// [`crate::Ui`] が生成したメニューを保持するため、戻り値を捨てても
/// 取り付け先から消えることはない。
#[derive(Clone)]
pub struct PopupMenu(Rc<PopupMenuInner>);

impl PopupMenu {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let element: HtmlElement = create(doc, "div")?.unchecked_into();
        element
            .set_attribute("role", "menu")
            .map_err(|e| to_error("メニューの組み立て", e))?;
        // 位置決めと重なりだけを CSS で作る。色はシステムカラーに任せる。
        style(&element, "position", "fixed");
        style(&element, "display", "none");
        style(&element, "z-index", "1000");
        style(&element, "min-width", "10em");
        style(&element, "padding", "4px");
        style(&element, "background", "Canvas");
        style(&element, "color", "CanvasText");
        style(&element, "border", "1px solid CanvasText");
        style(&element, "border-radius", "6px");
        style(&element, "box-shadow", "0 4px 12px rgba(0, 0, 0, 0.25)");

        let body = doc
            .body()
            .ok_or_else(|| Error::new("メニューの配置", "body 要素がありません"))?;
        body.append_child(&element)
            .map_err(|e| to_error("メニューの配置", e))?;

        let this = Self(Rc::new(PopupMenuInner {
            document: doc.clone(),
            element,
            buttons: RefCell::new(Vec::new()),
            item_listeners: RefCell::new(Vec::new()),
            attached: RefCell::new(Vec::new()),
            _dismiss: RefCell::new(Vec::new()),
            handler: SelectHandler::default(),
            open: Cell::new(false),
        }));
        this.install_dismiss(doc)?;
        Ok(this)
    }

    /// メニューの外側を押したときと Escape で閉じるようにする。
    fn install_dismiss(&self, doc: &Document) -> Result<()> {
        let target: web_sys::EventTarget = doc.clone().unchecked_into();

        let outside = Listener::attach_event(&target, "pointerdown", {
            let weak = Rc::downgrade(&self.0);
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                if !inner.open.get() {
                    return;
                }
                let node = event.target().and_then(|t| t.dyn_into::<Node>().ok());
                if !inner.element.contains(node.as_ref()) {
                    PopupMenu(inner).close();
                }
            }
        })?;

        let escape = Listener::attach_event(&target, "keydown", {
            let weak = Rc::downgrade(&self.0);
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                if !inner.open.get() {
                    return;
                }
                let is_escape = event
                    .dyn_ref::<KeyboardEvent>()
                    .map(|e| e.key() == "Escape")
                    .unwrap_or(false);
                if is_escape {
                    PopupMenu(inner).close();
                }
            }
        })?;

        *self.0._dismiss.borrow_mut() = vec![outside, escape];
        Ok(())
    }

    /// 項目を作り直す。以前の項目は取り除かれる。
    ///
    /// インデックスは区切り線を含めた並びの位置。
    pub fn set_items(&self, items: &[PopupItem]) {
        self.0.element.set_inner_html("");
        self.0.item_listeners.borrow_mut().clear();

        let doc = &self.0.document;
        let mut buttons = Vec::with_capacity(items.len());
        let mut listeners = Vec::new();
        for (index, item) in items.iter().enumerate() {
            if item.is_separator() {
                if let Ok(separator) = create(doc, "div") {
                    let separator: HtmlElement = separator.unchecked_into();
                    let _ = separator.set_attribute("role", "separator");
                    style(&separator, "height", "1px");
                    style(&separator, "margin", "4px 0");
                    style(&separator, "background", "currentColor");
                    style(&separator, "opacity", "0.25");
                    let _ = self.0.element.append_child(&separator);
                }
                buttons.push(None);
                continue;
            }

            let Ok(button) = create(doc, "button") else {
                buttons.push(None);
                continue;
            };
            let button: HtmlElement = button.unchecked_into();
            let _ = button.set_attribute("type", "button");
            let _ = button.set_attribute("role", "menuitem");
            button.set_text_content(Some(&item.label));
            // 行として並ぶよう、ボタンの既定の見た目だけを外す。
            style(&button, "display", "block");
            style(&button, "width", "100%");
            style(&button, "padding", "6px 12px");
            style(&button, "border", "none");
            style(&button, "border-radius", "4px");
            style(&button, "background", "transparent");
            style(&button, "color", "inherit");
            style(&button, "font", "inherit");
            style(&button, "text-align", "start");
            if !item.enabled {
                set_disabled(&button, true);
                style(&button, "opacity", "0.5");
            }

            if let Ok(listener) = Listener::attach(button.as_ref(), "click", {
                let weak = Rc::downgrade(&self.0);
                move || {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    let menu = PopupMenu(inner);
                    menu.close();
                    menu.0.handler.emit(index);
                }
            }) {
                listeners.push(listener);
            }
            let _ = self.0.element.append_child(&button);
            buttons.push(Some(button));
        }

        *self.0.buttons.borrow_mut() = buttons;
        *self.0.item_listeners.borrow_mut() = listeners;
    }

    /// 区切り線を含めた項目数。
    pub fn len(&self) -> usize {
        self.0.buttons.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// ウィジェットの右クリックでこのメニューを出すようにする。
    ///
    /// ブラウザ既定のコンテキストメニューは、取り付けたウィジェットの上でだけ
    /// 抑止する (`preventDefault`)。それ以外の場所では今までどおり出る。
    pub fn attach(&self, widget: &dyn Widget) {
        let element = widget.native_element();
        let listener = Listener::attach_event(element.as_ref(), "contextmenu", {
            let weak = Rc::downgrade(&self.0);
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                event.prevent_default();
                let (x, y) = match event.dyn_ref::<MouseEvent>() {
                    Some(mouse) => (mouse.client_x() as f64, mouse.client_y() as f64),
                    None => (0.0, 0.0),
                };
                PopupMenu(inner).open_at_viewport(x, y);
            }
        });
        if let Ok(listener) = listener {
            self.0
                .attached
                .borrow_mut()
                .push((listener, widget.boxed_clone()));
        }
    }

    /// プログラムからメニューを出す。位置は `widget` の**左上から**の
    /// 論理ピクセル (y は下向き)。
    pub fn open_at(&self, widget: &dyn Widget, x: f64, y: f64) {
        let rect = widget.native_element().get_bounding_client_rect();
        self.open_at_viewport(rect.left() + x, rect.top() + y);
    }

    /// 表示領域 (ビューポート) の左上を原点として出す。
    fn open_at_viewport(&self, x: f64, y: f64) {
        let element = &self.0.element;
        style(element, "left", &format!("{x}px"));
        style(element, "top", &format!("{y}px"));
        style(element, "display", "block");
        self.0.open.set(true);
        self.keep_inside_viewport(x, y);
    }

    /// はみ出す位置に出すよう言われたら、画面の内側へ寄せる。
    fn keep_inside_viewport(&self, x: f64, y: f64) {
        let Some(window) = web_sys::window() else {
            return;
        };
        let element = &self.0.element;
        let width = element.offset_width() as f64;
        let height = element.offset_height() as f64;
        if let Some(limit) = window.inner_width().ok().and_then(|v| v.as_f64()) {
            if x + width > limit {
                style(element, "left", &format!("{}px", (limit - width).max(0.0)));
            }
        }
        if let Some(limit) = window.inner_height().ok().and_then(|v| v.as_f64()) {
            if y + height > limit {
                style(element, "top", &format!("{}px", (limit - height).max(0.0)));
            }
        }
    }

    /// 出ているメニューを閉じる。出ていなければ何もしない。
    pub fn close(&self) {
        style(&self.0.element, "display", "none");
        self.0.open.set(false);
    }

    /// ユーザーが選んだのと同じ経路で項目を選ぶ (テストや自動操作用)。
    ///
    /// 区切り線と、選べない項目は無視する。
    pub fn select(&self, index: usize) {
        let button = self.0.buttons.borrow().get(index).cloned().flatten();
        if let Some(button) = button {
            button.click();
        }
    }

    /// 項目が選ばれたときに、そのインデックスで呼ばれる。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// メニュー本体の DOM 要素。バックエンド固有の脱出口として公開している。
    pub fn native_element(&self) -> Element {
        self.0.element.clone().unchecked_into()
    }
}
