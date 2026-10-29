//! Navigation handles (DOM).
//!
//! Browsers have no "tab" or "nav bar" controls, so they are built from semantic
//! standard elements (`<nav>` / `<ol>` / `<a>` / `<button>`) and WAI-ARIA roles.
//! The look stays the browser's default; CSS is used only for Flexbox layout and the
//! bold "selected" state.
//!
//! | naui | DOM |
//! | --- | --- |
//! | `Tabs` | `<div role="tablist">` + `<button role="tab">` + `<div role="tabpanel">` |
//! | `Navbar` | `<nav>` + `<strong>` + `<button>` |
//! | `Dock` | `<nav>` + `<button>` (equal widths) |
//! | `Menu` | `<nav><ul><li><button>` |
//! | `Breadcrumbs` | `<nav><ol><li><a href>` |
//! | `Pagination` | `<nav>` + `<button>` |
//! | `Link` | `<a href target="_blank">` |

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{ui_strings, NavItem, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement};

use crate::layout::{apply_child_layout, fill_parent, mark_parent, ParentLayout};
use crate::to_error;
use crate::widgets::{create, impl_widget, set_disabled, Listener, Widget};

/// The "selected" handler of navigation widgets.
///
/// It shares a single replaceable closure. So that operating on another navigation
/// widget from one navigation widget's callback does not double-borrow, the closure
/// is taken out only for the duration of the call.
#[derive(Clone, Default)]
pub(crate) struct SelectHandler(Rc<RefCell<Option<Box<dyn FnMut(usize)>>>>);

impl SelectHandler {
    pub(crate) fn set(&self, f: impl FnMut(usize) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    pub(crate) fn emit(&self, index: usize) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(index);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

fn style(element: &HtmlElement, property: &str, value: &str) {
    let _ = element.style().set_property(property, value);
}

/// Creates a horizontal element. `tag` is `nav` for the outer frame and `div` when
/// nested.
fn row(doc: &Document, tag: &str, gap: &str) -> Result<HtmlElement> {
    let element: HtmlElement = create(doc, tag)?.unchecked_into();
    style(&element, "display", "flex");
    style(&element, "flex-direction", "row");
    style(&element, "align-items", "center");
    style(&element, "gap", gap);
    Ok(element)
}

fn append(parent: &Element, child: &Element) -> Result<()> {
    parent
        .append_child(child)
        .map(|_| ())
        .map_err(|e| to_error("DOM への追加", e))
}

/// How the items are laid out.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// Buttons directly under `<nav>`.
    Flat,
    /// Wrapped in `<li>` (the contents of `<ul>` / `<ol>`).
    ListItem,
    /// Wrapped in `<li>`, with separators from the second one on.
    Crumb,
}

// -------------------------------------------------------------------- Bar

/// An internal handle holding "a sequence of items + the one currently selected".
///
/// Shared by nav bars, docks, menus, breadcrumbs and pagination.
#[derive(Clone)]
struct Bar(Rc<BarInner>);

struct BarInner {
    /// The element items go into (`<nav>` or `<ul>` / `<ol>`).
    mount: HtmlElement,
    shape: Shape,
    equal_width: bool,
    buttons: RefCell<Vec<HtmlElement>>,
    /// The click subscription per item. Removed when the items are rebuilt.
    listeners: RefCell<Vec<Listener>>,
    handler: SelectHandler,
    selected: Cell<Option<usize>>,
}

impl Bar {
    fn new(mount: HtmlElement, shape: Shape, equal_width: bool) -> Self {
        Self(Rc::new(BarInner {
            mount,
            shape,
            equal_width,
            buttons: RefCell::new(Vec::new()),
            listeners: RefCell::new(Vec::new()),
            handler: SelectHandler::default(),
            selected: Cell::new(None),
        }))
    }

    fn set_items(&self, doc: &Document, items: &[NavItem]) -> Result<()> {
        self.0.mount.set_inner_html("");
        self.0.buttons.borrow_mut().clear();
        self.0.listeners.borrow_mut().clear();

        let mut buttons = Vec::with_capacity(items.len());
        let mut listeners = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let tag = if self.0.shape == Shape::Crumb {
                "a"
            } else {
                "button"
            };
            let button: HtmlElement = create(doc, tag)?.unchecked_into();
            button.set_text_content(Some(&item.label));
            if self.0.shape == Shape::Crumb {
                // `href` は選択に合わせて `mark_selected` が付け外しする。
                let _ = button
                    .set_attribute("aria-disabled", if item.enabled { "false" } else { "true" });
            } else {
                let _ = button.set_attribute("type", "button");
            }
            set_disabled(&button, !item.enabled);
            if self.0.equal_width {
                style(&button, "flex", "1");
            }

            // ハンドルを強く持つと購読との間で循環するため、弱参照にする。
            let listener = if self.0.shape == Shape::Crumb {
                Listener::attach_event(button.as_ref(), "click", {
                    let weak = Rc::downgrade(&self.0);
                    let enabled = item.enabled;
                    move |event| {
                        event.prevent_default();
                        if enabled {
                            if let Some(inner) = weak.upgrade() {
                                // いまいる場所はリンクではない (`href` を外している)。
                                if inner.selected.get() != Some(index) {
                                    Bar(inner).select(index);
                                }
                            }
                        }
                    }
                })?
            } else {
                Listener::attach(button.as_ref(), "click", {
                    let weak = Rc::downgrade(&self.0);
                    move || {
                        if let Some(inner) = weak.upgrade() {
                            Bar(inner).select(index);
                        }
                    }
                })?
            };
            listeners.push(listener);

            match self.0.shape {
                Shape::Flat => append(&self.0.mount, &button)?,
                Shape::ListItem | Shape::Crumb => {
                    let li: HtmlElement = create(doc, "li")?.unchecked_into();
                    if self.0.shape == Shape::Crumb {
                        // 区切り文字とリンクを同じ縦位置・間隔で並べる。
                        // インライン要素のベースライン任せだと、ブラウザや
                        // フォントによって区切り文字が上下にずれる。
                        style(&li, "display", "flex");
                        style(&li, "align-items", "center");
                        style(&li, "gap", "4px");
                    }
                    if self.0.shape == Shape::Crumb && index > 0 {
                        let separator = create(doc, "span")?;
                        separator.set_text_content(Some("›"));
                        let _ = separator.set_attribute("aria-hidden", "true");
                        append(&li, &separator)?;
                    }
                    append(&li, &button)?;
                    append(&self.0.mount, &li)?;
                }
            }
            buttons.push(button);
        }
        *self.0.buttons.borrow_mut() = buttons;
        *self.0.listeners.borrow_mut() = listeners;
        // 項目が変わればインデックスの意味も変わるので、選択は必ず外す。
        self.mark_selected(None);
        Ok(())
    }

    /// Expresses the selected state with ARIA attributes and the look.
    ///
    /// Buttons are made bold. For breadcrumbs, bold would change the text width on every
    /// reselection and shift the items after it, so the current position and items that
    /// cannot be selected have their `href` removed, making them plain text that is not
    /// a link (drawn the browser's default way).
    fn mark_selected(&self, index: Option<usize>) {
        for (i, button) in self.0.buttons.borrow().iter().enumerate() {
            let current = Some(i) == index;
            if current {
                let _ = button.set_attribute("aria-current", "page");
            } else {
                let _ = button.remove_attribute("aria-current");
            }
            if self.0.shape == Shape::Crumb {
                let enabled = button.get_attribute("aria-disabled").as_deref() != Some("true");
                if current || !enabled {
                    let _ = button.remove_attribute("href");
                } else {
                    let _ = button.set_attribute("href", "#");
                }
                continue;
            }
            let _ = button.set_attribute("aria-selected", if current { "true" } else { "false" });
            style(
                button,
                "font-weight",
                if current { "bold" } else { "normal" },
            );
        }
        self.0.selected.set(index);
    }

    fn len(&self) -> usize {
        self.0.buttons.borrow().len()
    }

    fn selected(&self) -> Option<usize> {
        self.0.selected.get()
    }

    fn set_selected(&self, index: usize) {
        if index < self.len() {
            self.mark_selected(Some(index));
        }
    }

    fn select(&self, index: usize) {
        if index < self.len() {
            self.mark_selected(Some(index));
            self.0.handler.emit(index);
        }
    }

    fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }
}

// ------------------------------------------------------------------- Tabs

struct TabsInner {
    /// The whole tabs widget (`<div>`).
    element: HtmlElement,
    /// The tab headings (`<div role="tablist">`).
    tablist: HtmlElement,
    /// The `<div>` holding the contents.
    panels: HtmlElement,
    document: Document,
    tabs: RefCell<Vec<HtmlElement>>,
    panes: RefCell<Vec<HtmlElement>>,
    listeners: RefCell<Vec<Listener>>,
    children: RefCell<Vec<Box<dyn Widget>>>,
    handler: SelectHandler,
    selected: Cell<Option<usize>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// Tabs. Holds each tab's content widget.
///
/// Browsers have no tab control, so it is composed of standard elements with
/// `role="tab"` / `role="tabpanel"`, and unselected contents are hidden with the
/// `hidden` attribute.
#[derive(Clone)]
pub struct Tabs(Rc<TabsInner>);
impl_widget!(Tabs, element);

impl Tabs {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let element: HtmlElement = create(doc, "div")?.unchecked_into();
        style(&element, "display", "flex");
        style(&element, "flex-direction", "column");
        style(&element, "gap", "8px");

        let tablist = row(doc, "div", "4px")?;
        let _ = tablist.set_attribute("role", "tablist");
        let panels: HtmlElement = create(doc, "div")?.unchecked_into();
        // 中身がタブの余りを受け取れるようにする (AppKit の NSTabView と同じ)。
        style(&panels, "flex-grow", "1");
        style(&panels, "display", "flex");
        style(&panels, "flex-direction", "column");
        mark_parent(
            &panels,
            ParentLayout::Flex(naui_core::Orientation::Vertical),
        );

        append(&element, &tablist)?;
        append(&element, &panels)?;

        Ok(Self(Rc::new(TabsInner {
            element,
            tablist,
            panels,
            document: doc.clone(),
            tabs: RefCell::new(Vec::new()),
            panes: RefCell::new(Vec::new()),
            listeners: RefCell::new(Vec::new()),
            children: RefCell::new(Vec::new()),
            handler: SelectHandler::default(),
            selected: Cell::new(None),
            file_drop: Default::default(),
        })))
    }

    /// Adds one tab. `child` becomes that tab's content.
    pub fn add_tab(&self, label: &str, child: &dyn Widget) {
        let doc = &self.0.document;
        let Ok(tab) = create(doc, "button") else {
            return;
        };
        let tab: HtmlElement = tab.unchecked_into();
        tab.set_text_content(Some(label));
        let _ = tab.set_attribute("type", "button");
        let _ = tab.set_attribute("role", "tab");

        let Ok(pane) = create(doc, "div") else {
            return;
        };
        let pane: HtmlElement = pane.unchecked_into();
        let _ = pane.set_attribute("role", "tabpanel");
        // `display` は show() で切り替える。ここで書くと `hidden` が効かなくなる。
        style(&pane, "flex-grow", "1");
        style(&pane, "flex-direction", "column");
        mark_parent(&pane, ParentLayout::Flex(naui_core::Orientation::Vertical));
        let content = child.native_element();
        if pane.append_child(&content).is_err() {
            return;
        }
        // タブの中身は、タブの表示領域いっぱいに広がる。
        fill_parent(&content);

        let index = self.0.tabs.borrow().len();
        // 押されたタブの位置は、並びが変わることがあるので押されてから調べる
        // (`remove_tab` の後もそのタブが指す先がずれない)。
        let Ok(listener) = Listener::attach(tab.as_ref(), "click", {
            let weak = Rc::downgrade(&self.0);
            let tab = tab.clone();
            move || {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let index = inner.tabs.borrow().iter().position(|other| other == &tab);
                if let Some(index) = index {
                    Tabs(inner).select(index);
                }
            }
        }) else {
            return;
        };
        if self.0.tablist.append_child(&tab).is_err() || self.0.panels.append_child(&pane).is_err()
        {
            return;
        }
        self.0.listeners.borrow_mut().push(listener);
        self.0.tabs.borrow_mut().push(tab);
        self.0.panes.borrow_mut().push(pane);
        self.0.children.borrow_mut().push(child.boxed_clone());

        // 最初のタブは開いた状態にする。
        if index == 0 {
            self.show(Some(0));
        } else {
            self.show(self.0.selected.get());
        }
        // 区画を文書へ入れて開いてから整える。先に整えると、ほかの入れ物から
        // 移した中身の Scroll が、表示領域の無いうちに位置を戻そうとして待った
        // まま残る (大きさが同じなら、そのあと ResizeObserver も呼ばれない)。
        apply_child_layout(
            &content,
            ParentLayout::Flex(naui_core::Orientation::Vertical),
        );
    }

    /// Shows only the selected tab.
    fn show(&self, index: Option<usize>) {
        for (i, tab) in self.0.tabs.borrow().iter().enumerate() {
            let current = Some(i) == index;
            let _ = tab.set_attribute("aria-selected", if current { "true" } else { "false" });
            style(tab, "font-weight", if current { "bold" } else { "normal" });
        }
        for (i, pane) in self.0.panes.borrow().iter().enumerate() {
            if Some(i) == index {
                let _ = pane.remove_attribute("hidden");
                style(pane, "display", "flex");
            } else {
                // `hidden` の `display: none` を上書きしないよう、指定を消す。
                let _ = pane.style().remove_property("display");
                let _ = pane.set_attribute("hidden", "");
            }
        }
        self.0.selected.set(index);
    }

    /// Removes one tab. Does nothing if out of range.
    ///
    /// When the selected tab is removed, the tab at the same position (or the last tab
    /// if there is none) is selected instead. Like [`set_selected`](Tabs::set_selected),
    /// this move is not notified.
    pub fn remove_tab(&self, index: usize) {
        if index >= self.len() {
            return;
        }
        let tab = self.0.tabs.borrow_mut().remove(index);
        let pane = self.0.panes.borrow_mut().remove(index);
        self.0.children.borrow_mut().remove(index);
        // 購読は Listener の Drop で外れる。
        self.0.listeners.borrow_mut().remove(index);
        tab.remove();
        pane.remove();

        let left = self.len();
        let selected = match self.0.selected.get() {
            _ if left == 0 => None,
            Some(current) if current == index => Some(index.min(left - 1)),
            Some(current) if current > index => Some(current - 1),
            other => other,
        };
        self.show(selected);
    }

    /// Removes every tab.
    pub fn clear(&self) {
        for tab in std::mem::take(&mut *self.0.tabs.borrow_mut()) {
            tab.remove();
        }
        for pane in std::mem::take(&mut *self.0.panes.borrow_mut()) {
            pane.remove();
        }
        self.0.children.borrow_mut().clear();
        self.0.listeners.borrow_mut().clear();
        self.show(None);
    }

    pub fn len(&self) -> usize {
        self.0.tabs.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn selected(&self) -> Option<usize> {
        self.0.selected.get()
    }

    /// Changes the selection without notifying.
    pub fn set_selected(&self, index: usize) {
        if index < self.len() {
            self.show(Some(index));
        }
    }

    /// Changes the selection through the same path as a user's choice (notifies).
    pub fn select(&self, index: usize) {
        if index < self.len() {
            self.show(Some(index));
            self.0.handler.emit(index);
        }
    }

    /// Called with the index when the tab changes.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }
}

/// Adds the common implementation of navigation widgets with items.
macro_rules! impl_item_bar {
    ($t:ty) => {
        impl $t {
            /// Rebuilds the items. Indices change meaning, so the selection is cleared.
            pub fn set_items(&self, items: &[NavItem]) {
                let _ = self.0.bar.set_items(&self.0.document, items);
            }

            pub fn len(&self) -> usize {
                self.0.bar.len()
            }

            pub fn is_empty(&self) -> bool {
                self.len() == 0
            }

            pub fn selected(&self) -> Option<usize> {
                self.0.bar.selected()
            }

            /// Changes the selection without notifying.
            pub fn set_selected(&self, index: usize) {
                self.0.bar.set_selected(index);
            }

            /// Changes the selection through the same path as a user's choice (notifies).
            pub fn select(&self, index: usize) {
                self.0.bar.select(index);
            }

            /// Called with the index when an item is chosen.
            pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
                self.0.bar.on_select(f);
            }
        }
    };
}

// ----------------------------------------------------------------- Navbar

struct NavbarInner {
    element: HtmlElement,
    title: HtmlElement,
    document: Document,
    bar: Bar,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// Horizontal navigation placed at the top of the screen (`<nav>`).
#[derive(Clone)]
pub struct Navbar(Rc<NavbarInner>);
impl_widget!(Navbar, element);
impl_item_bar!(Navbar);

impl Navbar {
    pub(crate) fn new(doc: &Document, title: &str) -> Result<Self> {
        let element = row(doc, "nav", "12px")?;
        let title_element: HtmlElement = create(doc, "strong")?.unchecked_into();
        title_element.set_text_content(Some(title));
        append(&element, &title_element)?;

        // 項目は別の入れ物に入れる。`set_items` は入れ物の中身を作り直すので、
        // 見出しと同じ要素に入れると見出しごと消えてしまう。
        let items = row(doc, "div", "4px")?;
        append(&element, &items)?;

        let bar = Bar::new(items, Shape::Flat, false);
        Ok(Self(Rc::new(NavbarInner {
            element,
            title: title_element,
            document: doc.clone(),
            bar,
            file_drop: Default::default(),
        })))
    }

    /// The heading at the left end.
    pub fn set_title(&self, title: &str) {
        self.0.title.set_text_content(Some(title));
    }

    pub fn title(&self) -> String {
        self.0.title.text_content().unwrap_or_default()
    }
}

// ------------------------------------------------------------------- Dock

struct DockInner {
    element: HtmlElement,
    document: Document,
    bar: Bar,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// Horizontal navigation placed at the bottom of the screen (equal widths,
/// `<nav>`).
///
/// **Placement is the app's job.** naui's layout has only vertical and horizontal
/// stacks, so it cannot be pinned to the bottom of the window. Put it last in a
/// vertical stack to move it toward the bottom.
#[derive(Clone)]
pub struct Dock(Rc<DockInner>);
impl_widget!(Dock, element);
impl_item_bar!(Dock);

impl Dock {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let element = row(doc, "nav", "4px")?;
        let bar = Bar::new(element.clone(), Shape::Flat, true);
        Ok(Self(Rc::new(DockInner {
            element,
            document: doc.clone(),
            bar,
            file_drop: Default::default(),
        })))
    }
}

// ------------------------------------------------------------------- Menu

struct MenuInner {
    element: HtmlElement,
    document: Document,
    bar: Bar,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// A vertical list of navigation items (`<nav><ul>`).
///
/// It is not a popup menu.
#[derive(Clone)]
pub struct Menu(Rc<MenuInner>);
impl_widget!(Menu, element);
impl_item_bar!(Menu);

impl Menu {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let element: HtmlElement = create(doc, "nav")?.unchecked_into();
        let list: HtmlElement = create(doc, "ul")?.unchecked_into();
        // 箇条書きの点を消すのはブラウザ既定の一覧表示に任せず、
        // ナビゲーションとして読ませるための最低限の指定。
        style(&list, "list-style", "none");
        style(&list, "margin", "0");
        style(&list, "padding", "0");
        style(&list, "display", "flex");
        style(&list, "flex-direction", "column");
        style(&list, "gap", "2px");
        append(&element, &list)?;

        let bar = Bar::new(list, Shape::ListItem, false);
        Ok(Self(Rc::new(MenuInner {
            element,
            document: doc.clone(),
            bar,
            file_drop: Default::default(),
        })))
    }
}

// ------------------------------------------------------------ Breadcrumbs

struct BreadcrumbsInner {
    element: HtmlElement,
    document: Document,
    bar: Bar,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// Breadcrumbs (`<nav><ol>`).
#[derive(Clone)]
pub struct Breadcrumbs(Rc<BreadcrumbsInner>);
impl_widget!(Breadcrumbs, element);

impl Breadcrumbs {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let element: HtmlElement = create(doc, "nav")?.unchecked_into();
        let _ = element.set_attribute("aria-label", ui_strings().breadcrumbs);
        let list: HtmlElement = create(doc, "ol")?.unchecked_into();
        style(&list, "list-style", "none");
        style(&list, "margin", "0");
        style(&list, "padding", "0");
        style(&list, "display", "flex");
        style(&list, "flex-direction", "row");
        style(&list, "align-items", "center");
        style(&list, "gap", "4px");
        append(&element, &list)?;

        let bar = Bar::new(list, Shape::Crumb, false);
        Ok(Self(Rc::new(BreadcrumbsInner {
            element,
            document: doc.clone(),
            bar,
            file_drop: Default::default(),
        })))
    }

    /// Rebuilds the hierarchy. The last item becomes the current position.
    pub fn set_items(&self, items: &[NavItem]) {
        let _ = self.0.bar.set_items(&self.0.document, items);
        if let Some(last) = items.len().checked_sub(1) {
            self.0.bar.set_selected(last);
        }
    }

    pub fn len(&self) -> usize {
        self.0.bar.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The current position. The last item by default.
    pub fn selected(&self) -> Option<usize> {
        self.0.bar.selected()
    }

    /// Changes the current position without notifying.
    pub fn set_selected(&self, index: usize) {
        self.0.bar.set_selected(index);
    }

    /// Changes the selection through the same path as a user's choice (notifies).
    pub fn select(&self, index: usize) {
        self.0.bar.select(index);
    }

    /// Called with the index when an item is chosen.
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.bar.on_select(f);
    }
}

// ------------------------------------------------------------- Pagination

struct PaginationInner {
    element: HtmlElement,
    document: Document,
    bar: Bar,
    /// The subscriptions for previous / next.
    _steps: RefCell<Vec<Listener>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// Pagination (`<nav>` + `<button>`).
///
/// Browsers have no equivalent control, so it is composed of previous / next buttons
/// and page number buttons laid out in a row.
#[derive(Clone)]
pub struct Pagination(Rc<PaginationInner>);
impl_widget!(Pagination, element);

impl Pagination {
    pub(crate) fn new(doc: &Document, page_count: usize) -> Result<Self> {
        let element = row(doc, "nav", "4px")?;
        let _ = element.set_attribute("aria-label", ui_strings().pagination);

        let prev: HtmlElement = create(doc, "button")?.unchecked_into();
        prev.set_text_content(Some("‹"));
        let _ = prev.set_attribute("type", "button");
        let numbers = row(doc, "div", "4px")?;
        let next: HtmlElement = create(doc, "button")?.unchecked_into();
        next.set_text_content(Some("›"));
        let _ = next.set_attribute("type", "button");

        append(&element, &prev)?;
        append(&element, &numbers)?;
        append(&element, &next)?;

        let bar = Bar::new(numbers, Shape::Flat, false);
        let this = Self(Rc::new(PaginationInner {
            element,
            document: doc.clone(),
            bar,
            _steps: RefCell::new(Vec::new()),
            file_drop: Default::default(),
        }));

        let mut steps = Vec::new();
        for (button, forward) in [(&prev, false), (&next, true)] {
            let listener = Listener::attach(button.as_ref(), "click", {
                let weak = Rc::downgrade(&this.0);
                move || {
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    let pager = Pagination(inner);
                    if forward {
                        pager.go_next();
                    } else {
                        pager.go_previous();
                    }
                }
            })?;
            steps.push(listener);
        }
        *this.0._steps.borrow_mut() = steps;

        this.set_page_count(page_count);
        Ok(this)
    }

    /// Changes the number of pages. The display starts at 1; the API starts at 0.
    pub fn set_page_count(&self, count: usize) {
        let items: Vec<NavItem> = (1..=count).map(|n| NavItem::new(n.to_string())).collect();
        let _ = self.0.bar.set_items(&self.0.document, &items);
        if count > 0 {
            self.0.bar.set_selected(0);
        }
    }

    pub fn page_count(&self) -> usize {
        self.0.bar.len()
    }

    /// The current page (starting at 0).
    pub fn page(&self) -> usize {
        self.0.bar.selected().unwrap_or(0)
    }

    /// Changes the page without notifying.
    pub fn set_page(&self, page: usize) {
        self.0.bar.set_selected(page);
    }

    /// Changes the page through the same path as a user's choice (notifies).
    pub fn select(&self, page: usize) {
        self.0.bar.select(page);
    }

    /// Goes to the previous page. Does nothing on the first page.
    pub fn go_previous(&self) {
        let page = self.page();
        if page > 0 {
            self.select(page - 1);
        }
    }

    /// Goes to the next page. Does nothing on the last page.
    pub fn go_next(&self) {
        let page = self.page();
        if page + 1 < self.page_count() {
            self.select(page + 1);
        }
    }

    /// Called with the page number (starting at 0) when the page changes.
    pub fn on_change(&self, f: impl FnMut(usize) + 'static) {
        self.0.bar.on_select(f);
    }
}

// ------------------------------------------------------------------- Link

struct LinkInner {
    element: HtmlElement,
    listener: RefCell<Option<Listener>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// A link (`<a>`).
///
/// If `href` is not empty it opens in a new tab (`target="_blank"`). Navigating in
/// the same tab would destroy the whole wasm app, so it matches macOS / Windows,
/// which open it in the browser.
#[derive(Clone)]
pub struct Link(Rc<LinkInner>);
impl_widget!(Link, element);

impl Link {
    pub(crate) fn new(doc: &Document, text: &str, href: &str) -> Result<Self> {
        let element: HtmlElement = create(doc, "a")?.unchecked_into();
        element.set_text_content(Some(text));
        let _ = element.set_attribute("target", "_blank");
        let _ = element.set_attribute("rel", "noopener noreferrer");
        let this = Self(Rc::new(LinkInner {
            element,
            listener: RefCell::new(None),
            file_drop: Default::default(),
        }));
        this.set_href(href);
        Ok(this)
    }

    pub fn text(&self) -> String {
        self.0.element.text_content().unwrap_or_default()
    }

    pub fn set_text(&self, text: &str) {
        self.0.element.set_text_content(Some(text));
    }

    pub fn href(&self) -> String {
        self.0.element.get_attribute("href").unwrap_or_default()
    }

    pub fn set_href(&self, href: &str) {
        if href.is_empty() {
            let _ = self.0.element.remove_attribute("href");
        } else {
            let _ = self.0.element.set_attribute("href", href);
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        // `<a>` に disabled は無いので、クリックできない状態を作る。
        if enabled {
            let _ = self.0.element.remove_attribute("aria-disabled");
            style(&self.0.element, "pointer-events", "auto");
        } else {
            let _ = self.0.element.set_attribute("aria-disabled", "true");
            style(&self.0.element, "pointer-events", "none");
        }
    }

    /// Called on click. Setting it again removes the previous one.
    ///
    /// Opening `href` is done by the browser itself.
    pub fn on_click(&self, f: impl FnMut() + 'static) {
        let listener = Listener::attach(self.0.element.as_ref(), "click", f).ok();
        *self.0.listener.borrow_mut() = listener;
    }

    /// Triggers a click (for tests and automation).
    pub fn click(&self) {
        self.0.element.click();
    }
}
