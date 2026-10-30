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
//! | Item with a check mark | `<button role="menuitemcheckbox">` |
//! | Submenu | `<div role="menu">` beside its heading |
//! | Separator | `<div role="separator">` |
//!
//! The items, check marks and submenus are built by `crate::menu_dom`, the same as
//! [`crate::MenuBar`]'s.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{Error, MenuHandler, MenuItem, PopupItem, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement, KeyboardEvent, MouseEvent, Node};

use crate::menu_dom::{style, Actions, Look, MenuDom};
use crate::to_error;
use crate::widgets::{create, Listener, Widget};

/// The look of the menu and its submenus.
const LOOK: Look = Look {
    surface: |menu| {
        // 位置決めと重なりだけを CSS で作る。色はシステムカラーに任せる。
        style(menu, "z-index", "1000");
        style(menu, "min-width", "10em");
        style(menu, "padding", "4px");
        style(menu, "background", "Canvas");
        style(menu, "color", "CanvasText");
        style(menu, "border", "1px solid CanvasText");
        style(menu, "border-radius", "6px");
        style(menu, "box-shadow", "0 4px 12px rgba(0, 0, 0, 0.25)");
    },
    item: |button| {
        // 行として並ぶよう、ボタンの既定の見た目だけを外す。
        style(button, "padding", "6px 12px");
        style(button, "border", "none");
        style(button, "border-radius", "4px");
        style(button, "background", "transparent");
        style(button, "color", "inherit");
        style(button, "font", "inherit");
    },
    separator: |separator| {
        style(separator, "height", "1px");
        style(separator, "margin", "4px 0");
        style(separator, "background", "currentColor");
        style(separator, "opacity", "0.25");
    },
};

struct PopupMenuInner {
    document: Document,
    /// The menu itself. Placed directly under `<body>`, with `display: none` while not
    /// shown.
    element: HtmlElement,
    /// The items as passed, with the current check states.
    items: RefCell<Vec<PopupItem>>,
    /// The items and submenus, looked up by path. Rebuilt with the items.
    dom: RefCell<MenuDom>,
    /// The `contextmenu` subscriptions and handles of the widgets it is attached to.
    attached: RefCell<Vec<(Listener, Box<dyn Widget>)>>,
    /// The subscriptions for closing on clicks outside the menu and Escape.
    _dismiss: RefCell<Vec<Listener>>,
    handler: MenuHandler,
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
            .map_err(|e| to_error("build the menu", e))?;
        (LOOK.surface)(&element);
        style(&element, "position", "fixed");
        style(&element, "display", "none");

        let body = doc
            .body()
            .ok_or_else(|| Error::new("place the menu", "there is no body element"))?;
        body.append_child(&element)
            .map_err(|e| to_error("place the menu", e))?;

        let this = Self(Rc::new(PopupMenuInner {
            document: doc.clone(),
            element,
            items: RefCell::new(Vec::new()),
            dom: RefCell::new(MenuDom::default()),
            attached: RefCell::new(Vec::new()),
            _dismiss: RefCell::new(Vec::new()),
            handler: MenuHandler::default(),
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
    pub fn set_items(&self, items: &[PopupItem]) {
        self.0.element.set_inner_html("");
        *self.0.dom.borrow_mut() = MenuDom::default();

        // ハンドルを強く持つと購読との間で循環するため、弱参照にする。
        let choose = |path: Vec<usize>| -> Box<dyn FnMut()> {
            let weak = Rc::downgrade(&self.0);
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    let menu = PopupMenu(inner);
                    menu.close();
                    menu.choose(&path);
                }
            })
        };
        let open = |path: Vec<usize>| -> Box<dyn FnMut()> {
            let weak = Rc::downgrade(&self.0);
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    inner.dom.borrow().open_to(&path);
                }
            })
        };
        let mut dom = MenuDom::default();
        let as_menu: Vec<MenuItem> = items.iter().cloned().map(MenuItem::from).collect();
        let _ = dom.build(
            &self.0.document,
            &self.0.element,
            &as_menu,
            &mut Vec::new(),
            true,
            &LOOK,
            &Actions {
                choose: &choose,
                open: &open,
            },
        );
        *self.0.dom.borrow_mut() = dom;
        *self.0.items.borrow_mut() = items.to_vec();
    }

    /// Flips a check item, then notifies.
    fn choose(&self, path: &[usize]) {
        let flipped = {
            let mut items = self.0.items.borrow_mut();
            PopupItem::at_mut(&mut items, path)
                .filter(|item| item.is_checkable())
                .map(|item| {
                    let on = !item.checked.unwrap_or(false);
                    item.checked = Some(on);
                    on
                })
        };
        if let Some(on) = flipped {
            self.0.dom.borrow().set_checked(path, on);
        }
        self.0.handler.emit(path);
    }

    /// The number of top-level items, separators included.
    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
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
        self.0.dom.borrow().open_to(&[]);
        self.0.open.set(false);
    }

    /// Turns the check mark of the item at `path` on or off, without notifying. Does
    /// nothing for items without a check mark.
    pub fn set_item_checked(&self, path: &[usize], checked: bool) {
        let mut items = self.0.items.borrow_mut();
        let Some(item) = PopupItem::at_mut(&mut items, path) else {
            return;
        };
        if !item.is_checkable() {
            return;
        }
        item.checked = Some(checked);
        drop(items);
        self.0.dom.borrow().set_checked(path, checked);
    }

    /// Whether the check mark of the item at `path` is on.
    pub fn is_item_checked(&self, path: &[usize]) -> bool {
        PopupItem::at(&self.0.items.borrow(), path)
            .is_some_and(|item| item.is_checkable() && item.checked == Some(true))
    }

    /// Chooses an item through the same path as a user's choice (for tests and
    /// automation).
    ///
    /// Separators, submenu headings and items that cannot be chosen are ignored.
    pub fn select(&self, path: &[usize]) {
        if !PopupItem::can_choose(&self.0.items.borrow(), path) {
            return;
        }
        let button = self.0.dom.borrow().buttons.get(path).cloned();
        if let Some(button) = button {
            button.click();
        }
    }

    /// Called with the item's path when an item is chosen.
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.handler.set(f);
    }

    /// The `<button>` of the item at `path` (submenu headings included). `None` for
    /// separators and out of range. Public as a backend-specific escape hatch.
    pub fn native_item(&self, path: &[usize]) -> Option<HtmlElement> {
        self.0.dom.borrow().buttons.get(path).cloned()
    }

    /// The `<div role="menu">` of the submenu whose heading is at `path`. `None` if it
    /// is not a submenu. Public as a backend-specific escape hatch.
    pub fn native_submenu(&self, path: &[usize]) -> Option<HtmlElement> {
        self.0.dom.borrow().submenus.get(path).cloned()
    }

    /// The menu's DOM element. Public as a backend-specific escape hatch.
    pub fn native_element(&self) -> Element {
        self.0.element.clone().unchecked_into()
    }
}
