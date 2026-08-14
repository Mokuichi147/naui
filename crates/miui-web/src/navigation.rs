//! ナビゲーション系のハンドル群 (DOM)。
//!
//! ブラウザには「タブ」や「ナビバー」というコントロールが無いため、
//! 意味づけのある標準要素 (`<nav>` / `<ol>` / `<a>` / `<button>`) と
//! WAI-ARIA のロールで組み立てる。見た目はブラウザ既定のままで、
//! CSS は Flexbox のレイアウトと「選択中」の太字にしか使わない。
//!
//! | miui | DOM |
//! | --- | --- |
//! | `Tabs` | `<div role="tablist">` + `<button role="tab">` + `<div role="tabpanel">` |
//! | `Navbar` | `<nav>` + `<strong>` + `<button>` |
//! | `Dock` | `<nav>` + `<button>` (等幅) |
//! | `Menu` | `<nav><ul><li><button>` |
//! | `Breadcrumbs` | `<nav><ol><li><a href>` |
//! | `Pagination` | `<nav>` + `<button>` |
//! | `Link` | `<a href target="_blank">` |

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use miui_core::{NavItem, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement};

use crate::layout::{apply_child_layout, fill_parent, mark_parent, ParentLayout};
use crate::to_error;
use crate::widgets::{create, impl_widget, set_disabled, Listener, Widget};

/// ナビゲーション系ウィジェットの「選択された」通知先。
///
/// 差し替え可能な 1 本のクロージャを共有で持つ。あるナビゲーションの
/// コールバックから別のナビゲーションを操作しても二重借用にならないよう、
/// 呼び出しの間だけクロージャを取り出す。
#[derive(Clone, Default)]
pub(crate) struct SelectHandler(Rc<RefCell<Option<Box<dyn FnMut(usize)>>>>);

impl SelectHandler {
    fn set(&self, f: impl FnMut(usize) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, index: usize) {
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

/// 横並びの要素を作る。`tag` は外枠なら `nav`、入れ子なら `div`。
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

/// 項目の並べ方。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// `<nav>` の直下にボタンを並べる。
    Flat,
    /// `<li>` で包む (`<ul>` / `<ol>` の中身)。
    ListItem,
    /// `<li>` で包み、2 つ目以降に区切りを入れる。
    Crumb,
}

// -------------------------------------------------------------------- Bar

/// 「項目の並び + いま選ばれているもの」を持つ内部ハンドル。
///
/// ナビバー・ドック・メニュー・パンくず・ページネーションが共有する。
#[derive(Clone)]
struct Bar(Rc<BarInner>);

struct BarInner {
    /// 項目を入れる要素 (`<nav>` か `<ul>` / `<ol>`)。
    mount: HtmlElement,
    shape: Shape,
    equal_width: bool,
    buttons: RefCell<Vec<HtmlElement>>,
    /// 項目ごとのクリック購読。項目を作り直すと外れる。
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
                let _ = button.set_attribute("href", "#");
                let _ = button.set_attribute(
                    "aria-disabled",
                    if item.enabled { "false" } else { "true" },
                );
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
                                Bar(inner).select(index);
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
                        separator.set_text_content(Some("/"));
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

    /// 選択状態を ARIA 属性と太字で表す。
    fn mark_selected(&self, index: Option<usize>) {
        for (i, button) in self.0.buttons.borrow().iter().enumerate() {
            let current = Some(i) == index;
            if self.0.shape != Shape::Crumb {
                let _ = button.set_attribute("aria-selected", if current { "true" } else { "false" });
            }
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
    /// タブ全体 (`<div>`)。
    element: HtmlElement,
    /// タブの見出し (`<div role="tablist">`)。
    tablist: HtmlElement,
    /// 中身を入れる `<div>`。
    panels: HtmlElement,
    document: Document,
    tabs: RefCell<Vec<HtmlElement>>,
    panes: RefCell<Vec<HtmlElement>>,
    listeners: RefCell<Vec<Listener>>,
    children: RefCell<Vec<Box<dyn Widget>>>,
    handler: SelectHandler,
    selected: Cell<Option<usize>>,
}

/// タブ。中身のウィジェットごと持つ。
///
/// ブラウザにタブのコントロールは無いため、`role="tab"` /
/// `role="tabpanel"` を付けた標準要素で構成し、選ばれていない中身は
/// `hidden` 属性で隠す。
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
        mark_parent(&panels, ParentLayout::Flex(miui_core::Orientation::Vertical));

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
        })))
    }

    /// タブを 1 枚追加する。`child` がそのタブの中身になる。
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
        mark_parent(&pane, ParentLayout::Flex(miui_core::Orientation::Vertical));
        let content = child.native_element();
        if pane.append_child(&content).is_err() {
            return;
        }
        // タブの中身は、タブの表示領域いっぱいに広がる。
        fill_parent(&content);
        apply_child_layout(&content, ParentLayout::Flex(miui_core::Orientation::Vertical));

        let index = self.0.tabs.borrow().len();
        let listener = Listener::attach(tab.as_ref(), "click", {
            let weak = Rc::downgrade(&self.0);
            move || {
                if let Some(inner) = weak.upgrade() {
                    Tabs(inner).select(index);
                }
            }
        });
        if self.0.tablist.append_child(&tab).is_err() || self.0.panels.append_child(&pane).is_err()
        {
            return;
        }
        if let Ok(listener) = listener {
            self.0.listeners.borrow_mut().push(listener);
        }
        self.0.tabs.borrow_mut().push(tab);
        self.0.panes.borrow_mut().push(pane);
        self.0.children.borrow_mut().push(child.boxed_clone());

        // 最初のタブは開いた状態にする。
        if index == 0 {
            self.show(Some(0));
        } else {
            self.show(self.0.selected.get());
        }
    }

    /// 選ばれているタブだけを表示する。
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

    pub fn len(&self) -> usize {
        self.0.tabs.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn selected(&self) -> Option<usize> {
        self.0.selected.get()
    }

    /// 通知せずに選択を変える。
    pub fn set_selected(&self, index: usize) {
        if index < self.len() {
            self.show(Some(index));
        }
    }

    /// ユーザーが選んだのと同じ経路で選択を変える (通知あり)。
    pub fn select(&self, index: usize) {
        if index < self.len() {
            self.show(Some(index));
            self.0.handler.emit(index);
        }
    }

    /// タブが切り替わったときに、そのインデックスで呼ばれる。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }
}

/// 項目を持つナビゲーションの共通実装を生やす。
macro_rules! impl_item_bar {
    ($t:ty) => {
        impl $t {
            /// 項目を作り直す。インデックスの意味が変わるため、選択は外れる。
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

            /// 通知せずに選択を変える。
            pub fn set_selected(&self, index: usize) {
                self.0.bar.set_selected(index);
            }

            /// ユーザーが選んだのと同じ経路で選択を変える (通知あり)。
            pub fn select(&self, index: usize) {
                self.0.bar.select(index);
            }

            /// 項目が選ばれたときに、そのインデックスで呼ばれる。
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
}

/// 画面上部に置く横並びのナビゲーション (`<nav>`)。
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
        })))
    }

    /// 左端の見出し。
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
}

/// 画面下部に置く横並びのナビゲーション (等幅、`<nav>`)。
///
/// **配置はアプリの責務**で、miui のレイアウトは縦横のスタックしか持たないため、
/// ウィンドウ下端への固定はできない。縦スタックの最後に置くと下端寄りになる。
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
        })))
    }
}

// ------------------------------------------------------------------- Menu

struct MenuInner {
    element: HtmlElement,
    document: Document,
    bar: Bar,
}

/// 縦に並ぶナビゲーション一覧 (`<nav><ul>`)。
///
/// ポップアップのメニューではない。
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
        })))
    }
}

// ------------------------------------------------------------ Breadcrumbs

struct BreadcrumbsInner {
    element: HtmlElement,
    document: Document,
    bar: Bar,
}

/// パンくず (`<nav><ol>`)。
#[derive(Clone)]
pub struct Breadcrumbs(Rc<BreadcrumbsInner>);
impl_widget!(Breadcrumbs, element);

impl Breadcrumbs {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let element: HtmlElement = create(doc, "nav")?.unchecked_into();
        let _ = element.set_attribute("aria-label", "パンくず");
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
        })))
    }

    /// 階層を作り直す。末尾がいまいる場所になる。
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

    /// いまいる場所。既定では末尾の項目。
    pub fn selected(&self) -> Option<usize> {
        self.0.bar.selected()
    }

    /// 通知せずにいまいる場所を変える。
    pub fn set_selected(&self, index: usize) {
        self.0.bar.set_selected(index);
    }

    /// ユーザーが選んだのと同じ経路で選択を変える (通知あり)。
    pub fn select(&self, index: usize) {
        self.0.bar.select(index);
    }

    /// 項目が選ばれたときに、そのインデックスで呼ばれる。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.bar.on_select(f);
    }
}

// ------------------------------------------------------------- Pagination

struct PaginationInner {
    element: HtmlElement,
    document: Document,
    bar: Bar,
    /// 前へ / 次への購読。
    _steps: RefCell<Vec<Listener>>,
}

/// ページ送り (`<nav>` + `<button>`)。
///
/// ブラウザに相当するコントロールが無いため、前へ / 次へのボタンと
/// ページ番号のボタンを並べて構成している。
#[derive(Clone)]
pub struct Pagination(Rc<PaginationInner>);
impl_widget!(Pagination, element);

impl Pagination {
    pub(crate) fn new(doc: &Document, page_count: usize) -> Result<Self> {
        let element = row(doc, "nav", "4px")?;
        let _ = element.set_attribute("aria-label", "ページ送り");

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

    /// ページ数を変える。表示は 1 始まり、API は 0 始まり。
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

    /// いまのページ (0 始まり)。
    pub fn page(&self) -> usize {
        self.0.bar.selected().unwrap_or(0)
    }

    /// 通知せずにページを変える。
    pub fn set_page(&self, page: usize) {
        self.0.bar.set_selected(page);
    }

    /// ユーザーが選んだのと同じ経路でページを変える (通知あり)。
    pub fn select(&self, page: usize) {
        self.0.bar.select(page);
    }

    /// 前のページへ。先頭にいるときは何もしない。
    pub fn go_previous(&self) {
        let page = self.page();
        if page > 0 {
            self.select(page - 1);
        }
    }

    /// 次のページへ。末尾にいるときは何もしない。
    pub fn go_next(&self) {
        let page = self.page();
        if page + 1 < self.page_count() {
            self.select(page + 1);
        }
    }

    /// ページが変わったときに、その番号 (0 始まり) で呼ばれる。
    pub fn on_change(&self, f: impl FnMut(usize) + 'static) {
        self.0.bar.on_select(f);
    }
}

// ------------------------------------------------------------------- Link

struct LinkInner {
    element: HtmlElement,
    listener: RefCell<Option<Listener>>,
}

/// リンク (`<a>`)。
///
/// `href` が空でなければ別タブで開く (`target="_blank"`)。
/// 同じタブへ遷移すると wasm のアプリごと破棄されてしまうため、
/// macOS / Windows がブラウザで開くのに合わせている。
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

    /// 押されたときに呼ばれる。設定し直すと以前のものは外れる。
    ///
    /// `href` を開くのはブラウザ自身が行う。
    pub fn on_click(&self, f: impl FnMut() + 'static) {
        let listener = Listener::attach(self.0.element.as_ref(), "click", f).ok();
        *self.0.listener.borrow_mut() = listener;
    }

    /// クリックを発生させる (テストや自動操作用)。
    pub fn click(&self) {
        self.0.element.click();
    }
}
