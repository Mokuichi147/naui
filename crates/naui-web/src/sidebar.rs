//! サイドバー (`<aside>` + `<nav>`)。
//!
//! ブラウザにサイドバーのコントロールは無いため、意味づけのある標準要素で
//! 組み立てる。`<aside>` (補足のランドマーク) の中に `<nav>` を置き、
//! まとまりごとに見出しと `<ul>` を並べる。項目は `<button>` で、選択中の
//! ものは `aria-current="page"` と太字で表す (`Menu` と同じ)。
//!
//! | naui | DOM |
//! | --- | --- |
//! | サイドバー全体 | naui の [`SplitView`](crate::SplitView) (start に `<aside>`、end に中身) |
//! | 項目の一覧 | `<nav>` + まとまりごとの `<ul>` |
//! | まとまりの見出し | `<div role="heading">` (`<ul>` の `aria-labelledby`) |
//! | 項目 | `<li><button>` (アイコンは naui 同梱の SVG) |
//!
//! 見た目はブラウザ既定のままで、CSS は並びにしか使わない。中身との境目は
//! `SplitView` の仕切りそのもので、ドラッグ (と矢印キー) で幅を変えられる。
//!
//! ブラウザには開閉のボタンも無いので、`aria-expanded` / `aria-controls` を
//! 持つ `<button>` を置く。場所は macOS と同じくツールバーの行の先頭で
//! ([`Window`](crate::Window) が置く。ツールバーが無ければボタンだけの行に
//! なる)、開閉してもボタンは動かず、中身も上下しない。図形だけはツールバーと
//! 同じく naui が持つ。
//!
//! ほかのバックエンドに合わせて [`Widget`](crate::Widget) にはせず、
//! [`Window::set_sidebar`](crate::Window::set_sidebar) でウィンドウに
//! 取り付ける。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{
    sidebar_item, sidebar_len, sidebar_rows, Result, SidebarItem, SidebarRow, SidebarSection,
    DEFAULT_SIDEBAR_WIDTH, SIDEBAR_MIN_WIDTH,
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

/// 開閉ボタンの図形 (左に区画のある窓)。
const TOGGLE_PATH: &str = "M4 5h16v14H4zM9 5v14";

/// ページ内で見出しの id を重ねないための通し番号。
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
    /// サイドバーと中身を分ける仕切り。これの要素をウィンドウの中へ置く。
    split: SplitView,
    aside: HtmlElement,
    nav: HtmlElement,
    /// ウィンドウの子を入れる `<div>`。
    slot: HtmlElement,
    /// 開閉ボタン。
    toggle: HtmlElement,
    /// 開閉ボタンのクリック購読。
    toggle_listener: RefCell<Option<Listener>>,
    on_collapse: crate::widgets::ValueHandler<bool>,
    sections: RefCell<Vec<SidebarSection>>,
    /// 項目の通し番号と同じ並びのボタン。
    buttons: RefCell<Vec<HtmlElement>>,
    listeners: RefCell<Vec<Listener>>,
    handler: SelectHandler,
    selected: Cell<Option<usize>>,
    id: usize,
}

/// ウィンドウの左に付けるサイドバー。
///
/// [`Window::set_sidebar`](crate::Window::set_sidebar) で取り付ける。
/// レイアウトには置かないので [`Widget`](crate::Widget) ではない。
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
        let _ = nav.set_attribute("aria-label", "サイドバー");
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
        let _ = toggle.set_attribute("aria-label", "サイドバー");
        let _ = toggle.set_attribute("title", "サイドバー");
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

    /// 項目をまとまりごとに並べる。呼ぶたびに置き換わる。
    ///
    /// 選ばれていた項目は、同じ通し番号がまだ選べるなら選ばれたまま残る
    /// (通知はしない)。
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

    /// 見出しの無いまとまり 1 つだけで並べる。
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

    /// まとまり 1 つぶんの `<ul>` を作って `<nav>` へ足す。
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

    /// 選択状態を ARIA 属性と太字で表す。
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

    /// まとまりをまたいだ項目の数。
    pub fn len(&self) -> usize {
        sidebar_len(&self.0.sections.borrow())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 選ばれている項目の通し番号。
    pub fn selected(&self) -> Option<usize> {
        self.0.selected.get()
    }

    /// 通知せずに選択を変える。範囲外・選べない項目は無視する。
    pub fn set_selected(&self, index: usize) {
        if self.is_selectable(index) {
            self.mark_selected(Some(index));
        }
    }

    /// 選択を外す (通知しない)。
    pub fn clear_selection(&self) {
        self.mark_selected(None);
    }

    /// 利用者が選んだのと同じように選び、通知する。
    ///
    /// 範囲外・選べない項目は無視する。すでに選ばれている項目でも通知する。
    pub fn select(&self, index: usize) {
        if self.is_selectable(index) {
            self.mark_selected(Some(index));
            self.0.handler.emit(index);
        }
    }

    /// 項目が選ばれたときの通知先。引数は通し番号。
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.handler.set(f);
    }

    /// サイドバーの幅 (CSS ピクセル)。既定は [`DEFAULT_SIDEBAR_WIDTH`]。
    pub fn set_width(&self, width: f64) {
        if !width.is_finite() || width <= 0.0 {
            return;
        }
        self.0.split.set_position(width.max(SIDEBAR_MIN_WIDTH));
    }

    /// サイドバーの幅。閉じていても開いたときの幅を返す。
    pub fn width(&self) -> f64 {
        self.0.split.position()
    }

    /// 利用者が仕切りで幅を変えるたび、変えた後の幅で呼ばれる。
    pub fn on_resize(&self, f: impl FnMut(f64) + 'static) {
        self.0.split.on_resize(f);
    }

    /// サイドバーを閉じる (`true`) か開く (`false`)。
    ///
    /// 閉じても項目と選択と幅は残る。閉じている間はサイドバーと仕切りを
    /// `hidden` で隠す。[`on_collapse`](Self::on_collapse) は呼ばない。
    pub fn set_collapsed(&self, collapsed: bool) {
        self.0.split.set_start_hidden(collapsed);
        let _ = self
            .0
            .toggle
            .set_attribute("aria-expanded", if collapsed { "false" } else { "true" });
    }

    /// 利用者がサイドバーを開閉したときの通知先。引数は閉じたかどうか。
    ///
    /// 開閉ボタンの操作で呼ばれ、[`set_collapsed`](Self::set_collapsed)
    /// では呼ばれない。
    pub fn on_collapse(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_collapse.set(f);
    }

    /// 開閉ボタン。ウィンドウがツールバーの行の先頭へ置く。バックエンド固有の脱出口。
    pub fn native_toggle_button(&self) -> Element {
        self.0.toggle.clone().unchecked_into()
    }

    /// 開閉ボタン (ウィンドウがツールバーの行へ置くため)。
    pub(crate) fn toggle(&self) -> HtmlElement {
        self.0.toggle.clone()
    }

    /// サイドバーが閉じているかどうか。
    pub fn is_collapsed(&self) -> bool {
        self.0.split.is_start_hidden()
    }

    /// サイドバーと中身の間の仕切り。バックエンド固有の脱出口。
    pub fn native_divider(&self) -> HtmlElement {
        self.0.split.native_divider()
    }

    /// サイドバーの `<aside>`。バックエンド固有の脱出口。
    pub fn native_element(&self) -> Element {
        self.0.aside.clone().unchecked_into()
    }

    /// ウィンドウの中へ置く外枠 (サイドバーと中身を分ける `SplitView`)。
    pub(crate) fn mount(&self) -> HtmlElement {
        self.0.split.native_element().unchecked_into()
    }

    /// 中身の側へウィンドウの子を置く。`None` なら空にする。
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
