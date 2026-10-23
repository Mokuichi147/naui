//! Popup (context) menus (DOM).
//!
//! Browsers have no standard API for showing menus (the `<menu>` element is just a
//! list, and the browser's default context menu cannot be replaced from
//! JavaScript). So this is the one place **composed from `<div role="menu">` and
//! `<button role="menuitem">`**.
//!
//! Colors use CSS system colors (`Canvas` / `CanvasText`), so they follow the
//! `color-scheme` that [`crate::Ui::set_theme`] sets on `<html>` as is. naui never
//! decides colors itself.
//!
//! | naui | DOM |
//! | --- | --- |
//! | `PopupMenu` | `<div role="menu">` (directly under `<body>`, `position: fixed`) |
//! | Item | `<button role="menuitem">` |
//! | Separator | `<div role="separator">` |

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
    /// The menu itself. Placed directly under `<body>`, with `display: none` while not
    /// shown.
    element: HtmlElement,
    /// The button per item. `None` at separator positions.
    buttons: RefCell<Vec<Option<HtmlElement>>>,
    /// The click subscriptions of items. Removed when the items are rebuilt.
    item_listeners: RefCell<Vec<Listener>>,
    /// The `contextmenu` subscriptions and handles of the widgets it is attached to.
    attached: RefCell<Vec<(Listener, Box<dyn Widget>)>>,
    /// The subscriptions for closing on clicks outside the menu and Escape.
    _dismiss: RefCell<Vec<Listener>>,
    handler: SelectHandler,
    open: Cell<bool>,
}

/// A popup (context) menu.
///
/// It is not a widget laid out on screen, so it is not a [`Widget`]. [`crate::Ui`]
/// holds the menus it creates, so dropping the return value does not remove it from
/// the widgets it is attached to.
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

    /// Makes it close on clicks outside the menu and Escape.
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
                let Some(key) = event.dyn_ref::<KeyboardEvent>() else {
                    return;
                };
                // 変換中の Esc は入力を取り消すためのものなので、IME へ渡す。
                if key.key() != "Escape" || key.is_composing() {
                    return;
                }
                // Safari は全画面のとき、ページが既定動作を止めない限り Esc を
                // 全画面の解除に使ってしまう ([`crate::Dialog`] と同じ理由)。
                event.prevent_default();
                PopupMenu(inner).close();
            }
        })?;

        *self.0._dismiss.borrow_mut() = vec![outside, escape];
        Ok(())
    }

    /// Rebuilds the items. The previous items are removed.
    ///
    /// The index is the position in the sequence including separators.
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

    /// The number of items, separators included.
    pub fn len(&self) -> usize {
        self.0.buttons.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Makes this menu appear on right-click on the widget.
    ///
    /// The browser's default context menu is suppressed (`preventDefault`) only over the
    /// attached widgets. Elsewhere it appears as before.
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

    /// Shows the menu from code. The position is in logical pixels **from the top
    /// left** of `widget` (y pointing down).
    pub fn open_at(&self, widget: &dyn Widget, x: f64, y: f64) {
        let rect = widget.native_element().get_bounding_client_rect();
        self.open_at_viewport(rect.left() + x, rect.top() + y);
    }

    /// Shows it with the top left of the viewport as origin.
    fn open_at_viewport(&self, x: f64, y: f64) {
        let element = &self.0.element;
        style(element, "left", &format!("{x}px"));
        style(element, "top", &format!("{y}px"));
        style(element, "display", "block");
        self.0.open.set(true);
        self.keep_inside_viewport(x, y);
    }

    /// When asked to show it at a position that would overflow, moves it inside the
    /// screen.
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

    /// Closes the menu being shown. Does nothing if none is shown.
    pub fn close(&self) {
        style(&self.0.element, "display", "none");
        self.0.open.set(false);
    }

    /// Chooses an item through the same path as a user's choice (for tests and
    /// automation).
    ///
    /// Separators and items that cannot be chosen are ignored.
    pub fn select(&self, index: usize) {
        let button = self.0.buttons.borrow().get(index).cloned().flatten();
        if let Some(button) = button {
            button.click();
        }
    }

    /// Called with the index when an item is chosen.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// The menu's DOM element. Public as a backend-specific escape hatch.
    pub fn native_element(&self) -> Element {
        self.0.element.clone().unchecked_into()
    }
}
