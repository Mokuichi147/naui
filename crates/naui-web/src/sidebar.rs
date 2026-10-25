//! The sidebar (`<aside>` + `<nav>`).
//!
//! Browsers have no sidebar control, so it is built from semantic standard
//! elements. A `<nav>` is placed inside an `<aside>` (the complementary landmark),
//! with a heading and a `<ul>` per group. Items are `<button>`s, and the selected
//! one is shown with `aria-current="page"` and bold (the same as `Menu`).
//!
//! | naui | DOM |
//! | --- | --- |
//! | The whole sidebar | naui's [`SplitView`](crate::SplitView) (`<aside>` at start, the content at end) |
//! | Item list | `<nav>` + a `<ul>` per group |
//! | Group heading | `<div role="heading">` (the `aria-labelledby` of the `<ul>`) |
//! | Item | `<li><button>` (icons are SVGs bundled with naui) |
//!
//! The look stays the browser's default; CSS is used only for arrangement. The
//! boundary with the content is the `SplitView`'s divider itself, and the width can
//! be changed by dragging (and with the arrow keys).
//!
//! Browsers have no collapse button either, so a `<button>` with `aria-expanded` /
//! `aria-controls` is placed. As on macOS, it goes at the start of the toolbar row
//! (placed by [`Window`](crate::Window); without a toolbar, it becomes a row with
//! just the button), and neither the button nor the content moves when it collapses
//! or expands. Only its shape is owned by naui, as with the toolbar.
//!
//! To match the other backends it is not a [`Widget`](crate::Widget); it is
//! attached to the window with [`Window::set_sidebar`](crate::Window::set_sidebar).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{
    sidebar_item, sidebar_len, sidebar_rows, ui_strings, Result, SidebarItem, SidebarRow,
    SidebarSection, DEFAULT_SIDEBAR_WIDTH, SIDEBAR_MIN_WIDTH,
};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement};

use crate::layout::{apply_child_layout, fill_parent, mark_parent, ParentLayout};
use crate::navigation::SelectHandler;
use crate::split_view::SplitView;
use crate::to_error;
use crate::toolbar::{icon_svg, path_svg};
use crate::widgets::Widget;
use crate::widgets::{create, set_disabled, Listener};

/// The shape of the collapse button (a window with a pane on the left).
const TOGGLE_PATH: &str = "M4 5h16v14H4zM9 5v14";

/// A running number so that heading ids do not collide within the page.
static NEXT_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn style(element: &HtmlElement, property: &str, value: &str) {
    let _ = element.style().set_property(property, value);
}

fn append(parent: &Element, child: &Element) -> Result<()> {
    parent
        .append_child(child)
        .map(|_| ())
        .map_err(|e| to_error("DOM への追加", e))
}

struct SidebarInner {
    document: Document,
    /// The divider separating the sidebar and the content. Its element is placed inside
    /// the window.
    split: SplitView,
    aside: HtmlElement,
    nav: HtmlElement,
    /// The `<div>` holding the window's child.
    slot: HtmlElement,
    /// The collapse button.
    toggle: HtmlElement,
    /// The collapse button's click subscription.
    toggle_listener: RefCell<Option<Listener>>,
    on_collapse: crate::widgets::ValueHandler<bool>,
    sections: RefCell<Vec<SidebarSection>>,
    /// The buttons, in the same order as the items' running numbers.
    buttons: RefCell<Vec<HtmlElement>>,
    listeners: RefCell<Vec<Listener>>,
    handler: SelectHandler,
    selected: Cell<Option<usize>>,
    id: usize,
}

/// A sidebar attached to the left of the window.
///
/// Attach it with [`Window::set_sidebar`](crate::Window::set_sidebar). It is not
/// placed in a layout, so it is not a [`Widget`](crate::Widget).
#[derive(Clone)]
pub struct Sidebar(Rc<SidebarInner>);

impl Sidebar {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let split = SplitView::new(doc, naui_core::Orientation::Horizontal)?;
        split.set_min_sizes(SIDEBAR_MIN_WIDTH, 0.0);
        split.set_position(DEFAULT_SIDEBAR_WIDTH);

        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let aside: HtmlElement = create(doc, "aside")?.unchecked_into();
        let _ = aside.set_attribute("id", &format!("naui-sidebar-{id}"));
        style(&aside, "box-sizing", "border-box");
        style(&aside, "display", "flex");
        style(&aside, "flex-direction", "column");
        style(&aside, "gap", "8px");
        style(&aside, "flex-grow", "1");
        style(&aside, "min-height", "0");
        style(&aside, "overflow-y", "auto");
        style(&aside, "padding", "8px");

        let nav: HtmlElement = create(doc, "nav")?.unchecked_into();
        let _ = nav.set_attribute("aria-label", ui_strings().sidebar);
        style(&nav, "display", "flex");
        style(&nav, "flex-direction", "column");
        style(&nav, "gap", "12px");
        append(&aside, &nav)?;
        append(&split.start_element(), &aside)?;

        let content: HtmlElement = create(doc, "div")?.unchecked_into();
        style(&content, "display", "flex");
        style(&content, "flex-direction", "column");
        style(&content, "flex-grow", "1");
        style(&content, "min-height", "0");

        let toggle: HtmlElement = create(doc, "button")?.unchecked_into();
        let _ = toggle.set_attribute("type", "button");
        let _ = toggle.set_attribute("aria-label", ui_strings().sidebar);
        let _ = toggle.set_attribute("title", ui_strings().sidebar);
        let _ = toggle.set_attribute("aria-controls", &format!("naui-sidebar-{id}"));
        let _ = toggle.set_attribute("aria-expanded", "true");
        style(&toggle, "display", "inline-flex");
        style(&toggle, "align-items", "center");
        style(&toggle, "flex-shrink", "0");
        append(&toggle, &path_svg(doc, TOGGLE_PATH)?)?;

        let slot: HtmlElement = create(doc, "div")?.unchecked_into();
        style(&slot, "display", "flex");
        style(&slot, "flex-direction", "column");
        style(&slot, "flex-grow", "1");
        style(&slot, "min-height", "0");
        mark_parent(&slot, ParentLayout::Flex(naui_core::Orientation::Vertical));
        append(&content, &slot)?;
        append(&split.end_element(), &content)?;

        let this = Self(Rc::new(SidebarInner {
            document: doc.clone(),
            split,
            aside,
            nav,
            slot,
            toggle,
            toggle_listener: RefCell::new(None),
            on_collapse: crate::widgets::ValueHandler::default(),
            sections: RefCell::new(Vec::new()),
            buttons: RefCell::new(Vec::new()),
            listeners: RefCell::new(Vec::new()),
            handler: SelectHandler::default(),
            selected: Cell::new(None),
            id,
        }));

        // ハンドルを強く持つと購読との間で循環するため、弱参照にする。
        let listener = Listener::attach(this.0.toggle.as_ref(), "click", {
            let weak = Rc::downgrade(&this.0);
            move || {
                if let Some(inner) = weak.upgrade() {
                    let sidebar = Sidebar(inner);
                    let collapsed = !sidebar.is_collapsed();
                    sidebar.set_collapsed(collapsed);
                    sidebar.0.on_collapse.emit(collapsed);
                }
            }
        })?;
        *this.0.toggle_listener.borrow_mut() = Some(listener);
        Ok(this)
    }

    /// Lays out items by group. Each call replaces them.
    ///
    /// The selected item stays selected (without notifying) if the same running number
    /// can still be selected.
    pub fn set_sections(&self, sections: &[SidebarSection]) {
        let keep = self
            .0
            .selected
            .get()
            .filter(|&index| sidebar_item(sections, index).is_some_and(|item| item.enabled));
        *self.0.sections.borrow_mut() = sections.to_vec();
        let _ = self.rebuild(sections);
        self.mark_selected(keep);
    }

    /// Lays out items as a single group without a heading.
    pub fn set_items(&self, items: &[SidebarItem]) {
        self.set_sections(&[SidebarSection::untitled(items.iter().cloned())]);
    }

    fn rebuild(&self, sections: &[SidebarSection]) -> Result<()> {
        let doc = &self.0.document;
        self.0.nav.set_inner_html("");
        self.0.buttons.borrow_mut().clear();
        self.0.listeners.borrow_mut().clear();

        let mut buttons = Vec::new();
        let mut listeners = Vec::new();
        let mut list: Option<HtmlElement> = None;
        let mut group = 0;
        for row in sidebar_rows(sections) {
            match row {
                SidebarRow::Gap => list = None,
                SidebarRow::Header(title) => {
                    let heading: HtmlElement = create(doc, "div")?.unchecked_into();
                    let id = format!("naui-sidebar-{}-{group}", self.0.id);
                    let _ = heading.set_attribute("id", &id);
                    let _ = heading.set_attribute("role", "heading");
                    let _ = heading.set_attribute("aria-level", "2");
                    heading.set_text_content(Some(title));
                    // 見出しは項目より小さく淡く (「補助テキスト」の扱い)。
                    style(&heading, "font-size", "smaller");
                    style(&heading, "font-weight", "bold");
                    style(&heading, "color", "GrayText");
                    style(&heading, "padding", "0 4px");
                    append(&self.0.nav, &heading)?;
                    let ul = self.new_list()?;
                    let _ = ul.set_attribute("aria-labelledby", &id);
                    list = Some(ul);
                    group += 1;
                }
                SidebarRow::Item(index, item) => {
                    let ul = match &list {
                        Some(ul) => ul.clone(),
                        None => {
                            let ul = self.new_list()?;
                            list = Some(ul.clone());
                            ul
                        }
                    };
                    let li = create(doc, "li")?;
                    let (button, listener) = self.item_button(index, item)?;
                    append(&li, &button)?;
                    append(&ul, &li)?;
                    buttons.push(button);
                    listeners.push(listener);
                }
            }
        }
        *self.0.buttons.borrow_mut() = buttons;
        *self.0.listeners.borrow_mut() = listeners;
        Ok(())
    }

    /// Creates the `<ul>` for one group and adds it to the `<nav>`.
    fn new_list(&self) -> Result<HtmlElement> {
        let ul: HtmlElement = create(&self.0.document, "ul")?.unchecked_into();
        style(&ul, "list-style", "none");
        style(&ul, "margin", "0");
        style(&ul, "padding", "0");
        style(&ul, "display", "flex");
        style(&ul, "flex-direction", "column");
        style(&ul, "gap", "2px");
        append(&self.0.nav, &ul)?;
        Ok(ul)
    }

    fn item_button(&self, index: usize, item: &SidebarItem) -> Result<(HtmlElement, Listener)> {
        let doc = &self.0.document;
        let button: HtmlElement = create(doc, "button")?.unchecked_into();
        let _ = button.set_attribute("type", "button");
        style(&button, "display", "flex");
        style(&button, "align-items", "center");
        style(&button, "gap", "6px");
        style(&button, "width", "100%");
        style(&button, "text-align", "start");
        if let Some(icon) = item.icon {
            let svg = icon_svg(doc, icon)?;
            // 文字を省略するときも、記号は縮めない。
            let _ = svg.set_attribute("style", "flex-shrink: 0");
            append(&button, &svg)?;
        }
        let label: HtmlElement = create(doc, "span")?.unchecked_into();
        label.set_text_content(Some(&item.label));
        // 入りきらないときは、ほかの 3 環境と同じく 1 行のまま末尾を省略記号に
        // する (既定では折り返して行が高くなる)。
        style(&label, "min-width", "0");
        style(&label, "white-space", "nowrap");
        style(&label, "overflow", "hidden");
        style(&label, "text-overflow", "ellipsis");
        append(&button, &label)?;
        set_disabled(&button, !item.enabled);

        // ハンドルを強く持つと購読との間で循環するため、弱参照にする。
        let listener = Listener::attach(button.as_ref(), "click", {
            let weak = Rc::downgrade(&self.0);
            move || {
                if let Some(inner) = weak.upgrade() {
                    Sidebar(inner).select(index);
                }
            }
        })?;
        Ok((button, listener))
    }

    /// Expresses the selected state with ARIA attributes and bold.
    fn mark_selected(&self, index: Option<usize>) {
        for (i, button) in self.0.buttons.borrow().iter().enumerate() {
            let current = Some(i) == index;
            if current {
                let _ = button.set_attribute("aria-current", "page");
            } else {
                let _ = button.remove_attribute("aria-current");
            }
            style(
                button,
                "font-weight",
                if current { "bold" } else { "normal" },
            );
        }
        self.0.selected.set(index);
    }

    fn is_selectable(&self, index: usize) -> bool {
        sidebar_item(&self.0.sections.borrow(), index).is_some_and(|item| item.enabled)
    }

    /// The number of items across groups.
    pub fn len(&self) -> usize {
        sidebar_len(&self.0.sections.borrow())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The running number of the selected item.
    pub fn selected(&self) -> Option<usize> {
        self.0.selected.get()
    }

    /// Changes the selection without notifying. Out-of-range and unselectable items are
    /// ignored.
    pub fn set_selected(&self, index: usize) {
        if self.is_selectable(index) {
            self.mark_selected(Some(index));
        }
    }

    /// Clears the selection (without notifying).
    pub fn clear_selection(&self) {
        self.mark_selected(None);
    }

    /// Selects as if the user had chosen it, and notifies.
    ///
    /// Out-of-range and unselectable items are ignored. Notifies even if the item is
    /// already selected.
    pub fn select(&self, index: usize) {
        if self.is_selectable(index) {
            self.mark_selected(Some(index));
            self.0.handler.emit(index);
        }
    }

    /// The handler called when an item is chosen. The argument is the running number.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// The sidebar width (CSS pixels). The default is [`DEFAULT_SIDEBAR_WIDTH`].
    pub fn set_width(&self, width: f64) {
        if !width.is_finite() || width <= 0.0 {
            return;
        }
        self.0.split.set_position(width.max(SIDEBAR_MIN_WIDTH));
    }

    /// The sidebar width. Even while collapsed, returns the width it has when expanded.
    pub fn width(&self) -> f64 {
        self.0.split.position()
    }

    /// Called with the new width every time the user changes it with the divider.
    pub fn on_resize(&self, f: impl FnMut(f64) + 'static) {
        self.0.split.on_resize(f);
    }

    /// Collapses (`true`) or expands (`false`) the sidebar.
    ///
    /// Items, selection and width are kept while collapsed. While collapsed, the sidebar
    /// and the divider are hidden with `hidden`. [`on_collapse`](Self::on_collapse) is
    /// not called.
    pub fn set_collapsed(&self, collapsed: bool) {
        self.0.split.set_start_hidden(collapsed);
        let _ = self
            .0
            .toggle
            .set_attribute("aria-expanded", if collapsed { "false" } else { "true" });
    }

    /// The handler called when the user collapses or expands the sidebar. The argument
    /// is whether it is now collapsed.
    ///
    /// Called when the collapse button is used; not called by
    /// [`set_collapsed`](Self::set_collapsed).
    pub fn on_collapse(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_collapse.set(f);
    }

    /// The collapse button. The window places it at the start of the toolbar row. A
    /// backend-specific escape hatch.
    pub fn native_toggle_button(&self) -> Element {
        self.0.toggle.clone().unchecked_into()
    }

    /// The collapse button (for the window to place in the toolbar row).
    pub(crate) fn toggle(&self) -> HtmlElement {
        self.0.toggle.clone()
    }

    /// Whether the sidebar is collapsed.
    pub fn is_collapsed(&self) -> bool {
        self.0.split.is_start_hidden()
    }

    /// The divider between the sidebar and the content. A backend-specific escape
    /// hatch.
    pub fn native_divider(&self) -> HtmlElement {
        self.0.split.native_divider()
    }

    /// The sidebar's `<aside>`. A backend-specific escape hatch.
    pub fn native_element(&self) -> Element {
        self.0.aside.clone().unchecked_into()
    }

    /// The outer frame placed inside the window (the `SplitView` separating the sidebar
    /// and the content).
    pub(crate) fn mount(&self) -> HtmlElement {
        self.0.split.native_element().unchecked_into()
    }

    /// Places the window's child on the content side. `None` empties it.
    pub(crate) fn set_content(&self, element: Option<&Element>) {
        self.0.slot.set_inner_html("");
        if let Some(element) = element {
            if self.0.slot.append_child(element).is_ok() {
                fill_parent(element);
                apply_child_layout(
                    element,
                    ParentLayout::Flex(naui_core::Orientation::Vertical),
                );
            }
        }
    }
}
