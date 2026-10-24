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
//! | Separator | `<div role="separator">` |
//!
//! Colors use CSS system colors (`Canvas` / `CanvasText`), so they follow the
//! `color-scheme` that [`crate::Ui::set_theme`] sets on `<html>` as is (the same as
//! [`crate::PopupMenu`]).
//!
//! Shortcuts are also a concept browsers do not have, so naui watches `document`'s
//! `keydown` and matches them itself. The primary modifier can be either Ctrl or ⌘
//! (since ⌘ is the primary modifier in browsers on macOS). The right edge of items
//! shows [`MenuShortcut::label`](naui_core::MenuShortcut::label).
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

use naui_core::{MenuSpec, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement, KeyboardEvent, Node};

use crate::to_error;
use crate::widgets::{create, set_disabled, Listener};

fn style(element: &HtmlElement, property: &str, value: &str) {
    let _ = element.style().set_property(property, value);
}

/// The handler for the clicked item.
///
/// The closure is taken out only for the duration of the call, so rebuilding the
/// same menu bar from inside the notification does not double-borrow.
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

/// The DOM for one heading.
struct MenuParts {
    /// The heading's button.
    title: HtmlElement,
    /// The menu itself, shown below the heading.
    popup: HtmlElement,
    /// The button per item. `None` at separator positions.
    items: Vec<Option<HtmlElement>>,
}

struct MenuBarInner {
    element: HtmlElement,
    document: Document,
    menus: RefCell<Vec<MenuSpec>>,
    parts: RefCell<Vec<MenuParts>>,
    /// The click subscriptions of headings and items. Removed when the menus are
    /// rebuilt.
    listeners: RefCell<Vec<Listener>>,
    /// The subscriptions for clicks outside, Escape and shortcuts. Held only while
    /// attached to a window.
    document_listeners: RefCell<Vec<Listener>>,
    /// The element of the window it is attached to. Decides which window shortcuts are
    /// received for.
    window: RefCell<Option<Element>>,
    handler: Handler,
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
/// [`Window::set_menu_bar`](crate::Window::set_menu_bar). Every time an item is
/// clicked, [`on_activate`](Self::on_activate) is called with its **(heading index,
/// item index)**. The item index is the position in the sequence including
/// separators; separators are never returned.
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
            document_listeners: RefCell::new(Vec::new()),
            window: RefCell::new(None),
            handler: Handler::default(),
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
                if !primary || !bar.accepts_keys_from(&event) {
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

    /// Rebuilds the menus. The previous menus are removed.
    ///
    /// The heading index is the position in the sequence passed, and the item index is
    /// the position in the sequence including separators.
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

    /// Enables or disables one item. Does nothing for separators or out of range.
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

    /// Whether the item can be clicked now. `false` for separators and out of range.
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
    }

    /// Closes the open menu. Does nothing if none is open.
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

    /// Runs the item as if the user had chosen it.
    ///
    /// Does nothing for separators, disabled items or out of range.
    pub fn activate(&self, menu: usize, item: usize) {
        if self.is_item_enabled(menu, item) {
            self.0.handler.emit(menu, item);
        }
    }

    /// Called with the heading and item indices when an item is clicked. Setting it
    /// again removes the previous callback.
    pub fn on_activate(&self, f: impl FnMut(usize, usize) + 'static) {
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

    /// The item's `<button>`. `None` for separators and out of range. Public as a
    /// backend-specific escape hatch.
    pub fn native_item(&self, menu: usize, item: usize) -> Option<HtmlElement> {
        self.0.parts.borrow().get(menu)?.items.get(item)?.clone()
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

/// The notation of `aria-keyshortcuts` (`Control+Shift+S`).
///
/// WAI-ARIA requires modifier names to match the DOM's `KeyboardEvent.key`, so it is
/// built separately from the display-oriented
/// [`MenuShortcut::label`](naui_core::MenuShortcut::label).
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
