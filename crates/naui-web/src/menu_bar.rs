//! The menu bar (`<div role="menubar">` + `<div role="menu">`).
//!
//! Browsers have no OS application menu, and the `<menu>` element is just a list,
//! so this is the one place **composed from elements with WAI-ARIA roles**.
//!
//! | naui | DOM |
//! | --- | --- |
//! | `MenuBar` | `<div role="menubar">` |
//! | Heading | `<button role="menuitem" aria-haspopup="true">` |
//! | Menu | `<div role="menu">` (below the heading, `position: absolute`) |
//! | Item | `<button role="menuitem">` |
//! | Item with a check mark | `<button role="menuitemcheckbox">` |
//! | Submenu | `<div role="menu">` beside its heading |
//! | Separator | `<div role="separator">` |
//!
//! The items, check marks and submenus are built by `crate::menu_dom`, the same as
//! [`crate::PopupMenu`]'s.
//!
//! Colors use CSS system colors (`Canvas` / `CanvasText`), so they follow the
//! `color-scheme` that [`crate::Ui::set_theme`] sets on `<html>` as is (the same as
//! [`crate::PopupMenu`]).
//!
//! Shortcuts are also a concept browsers do not have, so naui watches `document`'s
//! `keydown` and matches them itself, including function keys and items inside
//! submenus. The primary modifier can be either Ctrl or ⌘ (since ⌘ is the primary
//! modifier in browsers on macOS). The right edge of items shows
//! [`MenuShortcut::label`](naui_core::MenuShortcut::label).
//!
//! The `document` subscriptions are attached **only while attached to a window**,
//! so that a detached menu bar, or the menu bar of a closed (hidden) window, does
//! not keep reacting to pressed keys. When the page has several windows, only the
//! menu bar of the window the key bubbled up from receives it, and keys from places
//! belonging to no window (`<body>` and so on) are received only by the first match
//! (the match prevents the default action, and later subscriptions see that and
//! back off).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{MenuHandler, MenuSpec, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement, KeyboardEvent, Node};

use crate::menu_dom::{style, Actions, Look, MenuDom};
use crate::to_error;
use crate::widgets::{create, set_disabled, Listener};

/// The look of the menus under the headings.
const LOOK: Look = Look {
    surface: |menu| {
        // 位置決めと重なりだけを CSS で作る。色はシステムカラーに任せる。
        style(menu, "z-index", "1000");
        style(menu, "min-width", "12em");
        style(menu, "padding", "4px");
        style(menu, "background", "Canvas");
        style(menu, "color", "CanvasText");
        style(menu, "border", "1px solid CanvasText");
        style(menu, "border-radius", "6px");
        style(menu, "box-shadow", "0 4px 12px rgba(0, 0, 0, 0.25)");
    },
    item: |_| {},
    separator: |separator| {
        style(separator, "height", "1px");
        style(separator, "margin", "4px 0");
        style(separator, "background", "currentColor");
        style(separator, "opacity", "0.3");
    },
};

/// The DOM for one heading.
struct MenuParts {
    /// The heading's button.
    title: HtmlElement,
    /// The menu itself, shown below the heading.
    popup: HtmlElement,
}

struct MenuBarInner {
    element: HtmlElement,
    document: Document,
    menus: RefCell<Vec<MenuSpec>>,
    parts: RefCell<Vec<MenuParts>>,
    /// The items and submenus, looked up by path (heading index first).
    dom: RefCell<MenuDom>,
    /// The click subscriptions of headings. Removed when the menus are rebuilt.
    listeners: RefCell<Vec<Listener>>,
    /// The subscriptions for clicks outside, Escape and shortcuts. Held only while
    /// attached to a window.
    document_listeners: RefCell<Vec<Listener>>,
    /// The element of the window it is attached to. Decides which window shortcuts are
    /// received for.
    window: RefCell<Option<Element>>,
    handler: MenuHandler,
    /// The heading open now.
    open: Cell<Option<usize>>,
    /// Whether the whole menu bar is enabled. ANDed with the per-item settings.
    enabled: Cell<bool>,
}

fn append(parent: &Element, child: &Element) -> Result<()> {
    parent
        .append_child(child)
        .map(|_| ())
        .map_err(|e| to_error("DOM への追加", e))
}

/// The equivalent of the OS's application menu, attached to the top of the window.
///
/// Not a [`Widget`](crate::Widget). Attach it with
/// [`Window::set_menu_bar`](crate::Window::set_menu_bar). Items are identified by
/// **a path** (the heading index, then the item index at each level of submenus,
/// counting separators). Every time an item is clicked,
/// [`on_activate`](Self::on_activate) is called with its path. Separators and
/// submenu headings are never returned.
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
            dom: RefCell::new(MenuDom::default()),
            listeners: RefCell::new(Vec::new()),
            document_listeners: RefCell::new(Vec::new()),
            window: RefCell::new(None),
            handler: MenuHandler::default(),
            open: Cell::new(None),
            enabled: Cell::new(true),
        }));
        Ok(this)
    }

    /// Called when attached to a window. Used only by [`crate::Window`].
    ///
    /// Attaches the subscriptions that close on outside clicks and Escape, and pick up
    /// shortcuts.
    pub(crate) fn attach(&self, window: &Element) {
        *self.0.window.borrow_mut() = Some(window.clone());
        if self.0.document_listeners.borrow().is_empty() {
            let _ = self.install_document_listeners();
        }
    }

    /// Called when detached from a window. Used only by [`crate::Window`].
    ///
    /// The `document` subscriptions are removed, so it no longer reacts to shortcuts.
    pub(crate) fn detach(&self) {
        self.close();
        self.0.document_listeners.borrow_mut().clear();
        *self.0.window.borrow_mut() = None;
    }

    /// Whether this menu bar may receive the pressed key (shortcuts and Escape).
    ///
    /// It does not receive keys while it is not on screen (its window was closed) or
    /// keys that bubbled up from another window. Keys it does not receive do not have
    /// their default action prevented either.
    fn accepts_keys_from(&self, event: &web_sys::Event) -> bool {
        // `display: none` の祖先を持つ要素は `offsetParent` を持たない。
        if !self.0.element.is_connected() || self.0.element.offset_parent().is_none() {
            return false;
        }
        let window = self.0.window.borrow();
        let Some(window) = window.as_ref() else {
            return false;
        };
        let owner = event
            .target()
            .and_then(|t| t.dyn_into::<Element>().ok())
            .and_then(|element| {
                element
                    .closest(crate::window::WINDOW_SELECTOR)
                    .ok()
                    .flatten()
            });
        match owner {
            // キーが上がってきたウィンドウのメニューバーだけが受け取る。
            Some(owner) => &owner == window,
            // どのウィンドウにも属さないところからのキーは、誰もまだ
            // 受け取っていなければ受け取る。
            None => !event.default_prevented(),
        }
    }

    /// Makes it close on outside clicks and Escape, and pick up shortcuts.
    fn install_document_listeners(&self) -> Result<()> {
        let target: web_sys::EventTarget = self.0.document.clone().unchecked_into();

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
                    // ショートカットと同じく、画面に出ていないときと、別の
                    // ウィンドウから上がってきたキーには触らない。
                    if bar.0.open.get().is_some() && bar.accepts_keys_from(&event) {
                        // Safari は全画面のとき、ページが既定動作を止めない
                        // 限り Esc を全画面の解除に使ってしまう。
                        event.prevent_default();
                        bar.close();
                    }
                    return;
                }
                // 主修飾キーは Ctrl と ⌘ のどちらでもよい。
                let primary = key.ctrl_key() || key.meta_key();
                if !bar.accepts_keys_from(&event) {
                    return;
                }
                if let Some(path) =
                    bar.find_shortcut(&key.key(), primary, key.shift_key(), key.alt_key())
                {
                    // ブラウザの既定のショートカット (⌘S や F5 など) は起こさない。
                    event.prevent_default();
                    bar.close();
                    bar.choose(&path);
                }
            }
        })?;

        *self.0.document_listeners.borrow_mut() = vec![outside, keys];
        Ok(())
    }

    /// Looks for a currently clickable item matching the pressed key.
    ///
    /// The result is taken out before returning the borrow (so that rebuilding the
    /// menus inside the notification does not double-borrow).
    fn find_shortcut(
        &self,
        key: &str,
        primary: bool,
        shift: bool,
        alt: bool,
    ) -> Option<Vec<usize>> {
        if !self.0.enabled.get() {
            return None;
        }
        MenuSpec::find_shortcut(&self.0.menus.borrow(), |s| {
            s.matches(key, primary, shift, alt)
        })
    }

    /// Rebuilds the menus. The previous menus are removed.
    pub fn set_menus(&self, menus: &[MenuSpec]) {
        let _ = self.rebuild(menus);
    }

    fn rebuild(&self, menus: &[MenuSpec]) -> Result<()> {
        let doc = self.0.document.clone();
        self.0.element.set_inner_html("");
        self.0.parts.borrow_mut().clear();
        self.0.listeners.borrow_mut().clear();
        *self.0.dom.borrow_mut() = MenuDom::default();
        self.0.open.set(None);

        let whole = self.0.enabled.get();
        let mut parts = Vec::with_capacity(menus.len());
        let mut listeners = Vec::new();
        let mut dom = MenuDom::default();
        // ハンドルを強く持つと購読との間で循環するため、弱参照にする。
        let choose = |path: Vec<usize>| -> Box<dyn FnMut()> {
            let weak = Rc::downgrade(&self.0);
            Box::new(move || {
                if let Some(inner) = weak.upgrade() {
                    let bar = MenuBar(inner);
                    bar.close();
                    bar.choose(&path);
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
        let actions = Actions {
            choose: &choose,
            open: &open,
        };
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
            (LOOK.surface)(&popup);
            style(&popup, "position", "absolute");
            style(&popup, "top", "100%");
            style(&popup, "left", "0");
            style(&popup, "display", "none");
            dom.build(
                &doc,
                &popup,
                &spec.items,
                &mut vec![menu_index],
                whole,
                &LOOK,
                &actions,
            )?;

            append(&holder, &popup)?;
            append(&self.0.element, &holder)?;
            parts.push(MenuParts { title, popup });
        }

        *self.0.parts.borrow_mut() = parts;
        *self.0.listeners.borrow_mut() = listeners;
        *self.0.dom.borrow_mut() = dom;
        let mut stored = self.0.menus.borrow_mut();
        stored.clear();
        stored.extend_from_slice(menus);
        Ok(())
    }

    /// Flips a check item, then notifies. The same path for the user's choice,
    /// shortcuts and `activate`.
    fn choose(&self, path: &[usize]) {
        let flipped = {
            let mut menus = self.0.menus.borrow_mut();
            MenuSpec::item_at_mut(&mut menus, path)
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

    /// The number of headings.
    pub fn len(&self) -> usize {
        self.0.menus.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The number of items under one heading, separators included. `0` if out of
    /// range.
    pub fn menu_len(&self, menu: usize) -> usize {
        self.0
            .menus
            .borrow()
            .get(menu)
            .map_or(0, |spec| spec.items.len())
    }

    /// Enables or disables the item at `path`. Does nothing for separators or out of
    /// range.
    pub fn set_item_enabled(&self, path: &[usize], enabled: bool) {
        let mut menus = self.0.menus.borrow_mut();
        let Some(entry) = MenuSpec::item_at_mut(&mut menus, path) else {
            return;
        };
        if entry.is_separator() {
            return;
        }
        entry.enabled = enabled;
        drop(menus);
        self.apply_enabled();
    }

    /// Whether the item at `path` can be clicked now. `false` for separators, submenu
    /// headings and out of range.
    pub fn is_item_enabled(&self, path: &[usize]) -> bool {
        self.0.enabled.get() && MenuSpec::can_choose(&self.0.menus.borrow(), path)
    }

    /// Turns the check mark of the item at `path` on or off, without notifying. Does
    /// nothing for items without a check mark.
    pub fn set_item_checked(&self, path: &[usize], checked: bool) {
        let mut menus = self.0.menus.borrow_mut();
        let Some(entry) = MenuSpec::item_at_mut(&mut menus, path) else {
            return;
        };
        if !entry.is_checkable() {
            return;
        }
        entry.checked = Some(checked);
        drop(menus);
        self.0.dom.borrow().set_checked(path, checked);
    }

    /// Whether the check mark of the item at `path` is on.
    pub fn is_item_checked(&self, path: &[usize]) -> bool {
        MenuSpec::item_at(&self.0.menus.borrow(), path)
            .is_some_and(|entry| entry.is_checkable() && entry.checked == Some(true))
    }

    /// Enables or disables the whole menu bar. Per-item settings are kept.
    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        if !enabled {
            self.close();
        }
        self.apply_enabled();
    }

    /// Applies the per-item settings and the overall setting to the DOM.
    fn apply_enabled(&self) {
        let whole = self.0.enabled.get();
        for parts in self.0.parts.borrow().iter() {
            set_disabled(&parts.title, !whole);
        }
        let menus = self.0.menus.borrow();
        let dom = self.0.dom.borrow();
        MenuSpec::walk(&menus, |path, _| {
            if let Some(button) = dom.buttons.get(path) {
                set_disabled(button, !(whole && MenuSpec::enabled_at(&menus, path)));
            }
        });
    }

    /// Opening and closing when a heading is clicked.
    fn toggle(&self, menu: usize) {
        if self.0.open.get() == Some(menu) {
            self.close();
        } else {
            self.open(menu);
        }
    }

    /// Shows one heading's menu. Does nothing if out of range or disabled.
    ///
    /// The other three platforms have no way to "open a menu from code", so this is not
    /// public API and is used only when a heading is clicked.
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
        self.0.dom.borrow().open_to(&[menu]);
    }

    /// Closes the open menu. Does nothing if none is open.
    pub(crate) fn close(&self) {
        let Some(menu) = self.0.open.take() else {
            return;
        };
        self.0.dom.borrow().open_to(&[]);
        let parts = self.0.parts.borrow();
        if let Some(parts) = parts.get(menu) {
            style(&parts.popup, "display", "none");
            let _ = parts.title.set_attribute("aria-expanded", "false");
        }
    }

    /// Runs the item at `path` as if the user had chosen it.
    ///
    /// Does nothing for separators, submenu headings, disabled items or out of range.
    pub fn activate(&self, path: &[usize]) {
        if self.is_item_enabled(path) {
            self.choose(path);
        }
    }

    /// Called with the item's path when an item is clicked. Setting it again removes
    /// the previous callback.
    pub fn on_activate(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.handler.set(f);
    }

    /// The menu bar's `<div role="menubar">`. Public as a backend-specific escape
    /// hatch.
    pub fn native_element(&self) -> Element {
        self.0.element.clone().unchecked_into()
    }

    /// The heading's `<button>`. `None` if out of range. Public as a backend-specific
    /// escape hatch.
    pub fn native_title(&self, menu: usize) -> Option<HtmlElement> {
        Some(self.0.parts.borrow().get(menu)?.title.clone())
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

    /// The `<div role="menu">` shown below the heading. `None` if out of range. Public
    /// as a backend-specific escape hatch.
    pub fn native_menu(&self, menu: usize) -> Option<HtmlElement> {
        Some(self.0.parts.borrow().get(menu)?.popup.clone())
    }

    /// The element inserted into the window. Used only by [`crate::Window`].
    pub(crate) fn mount(&self) -> HtmlElement {
        self.0.element.clone()
    }
}
