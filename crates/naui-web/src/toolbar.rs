//! The toolbar (`<div role="toolbar">` + `<button>`).
//!
//! Browsers have no toolbar control, so buttons are laid out in a `<div>` with
//! WAI-ARIA's `role="toolbar"`, and separators are `<div>`s with
//! `role="separator"`. The look stays the browser's default; CSS is used only for
//! the horizontal layout and drawing separator lines.
//!
//! To match the other backends it is not a [`Widget`](crate::Widget); it is
//! attached to the top of the window with
//! [`Window::set_toolbar`](crate::Window::set_toolbar).
//!
//! **Browsers have no OS icon theme**, so for icons alone naui owns the shapes
//! ([`ToolbarIcon::svg_path`](naui_core::ToolbarIcon::svg_path)) and draws them as
//! inline SVG. The color follows `currentColor`, so it blends in with the browser's
//! and page's color scheme. `label` is used for screen readers and tooltips.

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
    /// In the same order as the items. `None` at separators.
    buttons: RefCell<Vec<Option<HtmlElement>>>,
    /// The click subscription per item. Removed when the items are rebuilt.
    listeners: RefCell<Vec<Listener>>,
    handler: Handler,
    /// Whether the whole toolbar is enabled. ANDed with the per-item settings.
    enabled: Cell<bool>,
}

/// The handler for the clicked item.
///
/// The closure is taken out only for the duration of the call, so rebuilding the
/// same toolbar from inside the notification does not double-borrow.
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

/// Creates the inline SVG for one icon. Sidebar items use it too.
pub(crate) fn icon_svg(doc: &Document, icon: ToolbarIcon) -> Result<Element> {
    path_svg(doc, icon.svg_path())
}

/// Creates the inline SVG for one line-drawn shape (24x24 coordinate space).
pub(crate) fn path_svg(doc: &Document, d: &str) -> Result<Element> {
    const NS: &str = "http://www.w3.org/2000/svg";
    let svg = doc
        .create_element_ns(Some(NS), "svg")
        .map_err(|e| to_error("create the SVG", e))?;
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
        .map_err(|e| to_error("create the SVG path", e))?;
    let _ = path.set_attribute("d", d);
    append(&svg, &path)?;
    Ok(svg)
}

fn append(parent: &Element, child: &Element) -> Result<()> {
    parent
        .append_child(child)
        .map(|_| ())
        .map_err(|e| to_error("add to the DOM", e))
}

/// A row of frequently used actions attached to the top of the window.
///
/// Not a [`Widget`](crate::Widget). Attach it with
/// [`Window::set_toolbar`](crate::Window::set_toolbar). Unlike navigation, it
/// **has no selected item**; every click calls [`on_activate`](Self::on_activate)
/// with the index. The index is the position in the sequence including separators;
/// separators are never returned.
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

    /// Rebuilds the items. The previous items are removed.
    ///
    /// The index is the position in the sequence including separators.
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

    /// The number of items, separators included.
    pub fn len(&self) -> usize {
        self.0.items.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Enables or disables one item. Does nothing for separators or out of range.
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

    /// Whether the item can be clicked now. `false` for separators and out of range.
    pub fn is_item_enabled(&self, index: usize) -> bool {
        self.0.enabled.get()
            && self
                .0
                .items
                .borrow()
                .get(index)
                .is_some_and(|item| !item.is_separator() && item.enabled)
    }

    /// Enables or disables the whole toolbar. Per-item settings are kept.
    pub fn set_enabled(&self, enabled: bool) {
        self.0.enabled.set(enabled);
        self.apply_enabled();
    }

    /// Applies the per-item settings and the overall setting to the DOM.
    fn apply_enabled(&self) {
        let whole = self.0.enabled.get();
        let items = self.0.items.borrow();
        for (button, item) in self.0.buttons.borrow().iter().zip(items.iter()) {
            if let Some(button) = button {
                set_disabled(button, !(item.enabled && whole));
            }
        }
    }

    /// Runs the item as if the user had clicked it.
    ///
    /// Does nothing for separators, disabled items or out of range.
    pub fn activate(&self, index: usize) {
        if self.is_item_enabled(index) {
            self.0.handler.emit(index);
        }
    }

    /// Called with the index when an item is clicked. Setting it again removes the
    /// previous callback.
    pub fn on_activate(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// The `<button>` for the item. `None` for separators and out of range. Public as a
    /// backend-specific escape hatch.
    pub fn native_button(&self, index: usize) -> Option<HtmlElement> {
        self.0.buttons.borrow().get(index)?.clone()
    }

    /// The toolbar's `<div role="toolbar">`. Public as a backend-specific escape hatch.
    pub fn native_element(&self) -> Element {
        self.0.element.clone().unchecked_into()
    }

    /// The element inserted into the window. Used only by [`crate::Window`].
    pub(crate) fn mount(&self) -> HtmlElement {
        self.0.element.clone()
    }
}
