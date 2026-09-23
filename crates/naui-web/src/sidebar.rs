//! サイドバー (`<aside>` + `<nav>`)。
//!
//! ブラウザにサイドバーのコントロールは無いため、意味づけのある標準要素で
//! 組み立てる。`<aside>` (補足のランドマーク) の中に `<nav>` を置き、
//! まとまりごとに見出しと `<ul>` を並べる。項目は `<button>` で、選択中の
//! ものは `aria-current="page"` と太字で表す (`Menu` と同じ)。
//!
//! | naui | DOM |
//! | --- | --- |
//! | サイドバー全体 | `<div>` (横並び) の中に `<aside>` と中身の `<div>` |
//! | 項目の一覧 | `<nav>` + まとまりごとの `<ul>` |
//! | まとまりの見出し | `<div role="heading">` (`<ul>` の `aria-labelledby`) |
//! | 項目 | `<li><button>` (アイコンは naui 同梱の SVG) |
//!
//! 見た目はブラウザ既定のままで、CSS は並びと、中身との境目の線にしか
//! 使わない。線の色は `SplitView` と同じシステムカラーの `GrayText`。
//!
//! ほかのバックエンドに合わせて [`Widget`](crate::Widget) にはせず、
//! [`Window::set_sidebar`](crate::Window::set_sidebar) でウィンドウに
//! 取り付ける。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{
    sidebar_item, sidebar_len, sidebar_rows, Result, SidebarItem, SidebarRow, SidebarSection,
    DEFAULT_SIDEBAR_WIDTH,
};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement};

use crate::layout::{apply_child_layout, fill_parent, mark_parent, ParentLayout};
use crate::navigation::SelectHandler;
use crate::to_error;
use crate::toolbar::icon_svg;
use crate::widgets::{create, set_disabled, Listener};

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
    /// サイドバーと中身を横に並べる外枠。ウィンドウの中へ置く。
    mount: HtmlElement,
    aside: HtmlElement,
    nav: HtmlElement,
    /// 中身 (ウィンドウの子) を入れる `<div>`。
    content: HtmlElement,
    sections: RefCell<Vec<SidebarSection>>,
    /// 項目の通し番号と同じ並びのボタン。
    buttons: RefCell<Vec<HtmlElement>>,
    listeners: RefCell<Vec<Listener>>,
    handler: SelectHandler,
    selected: Cell<Option<usize>>,
    width: Cell<f64>,
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
        let mount: HtmlElement = create(doc, "div")?.unchecked_into();
        style(&mount, "display", "flex");
        style(&mount, "flex-direction", "row");
        style(&mount, "min-height", "0");
        fill_parent(&mount);

        let aside: HtmlElement = create(doc, "aside")?.unchecked_into();
        style(&aside, "box-sizing", "border-box");
        style(&aside, "flex-shrink", "0");
        style(&aside, "overflow-y", "auto");
        style(&aside, "padding", "8px");
        style(&aside, "border-inline-end", "1px solid GrayText");

        let nav: HtmlElement = create(doc, "nav")?.unchecked_into();
        let _ = nav.set_attribute("aria-label", "サイドバー");
        style(&nav, "display", "flex");
        style(&nav, "flex-direction", "column");
        style(&nav, "gap", "12px");
        append(&aside, &nav)?;

        let content: HtmlElement = create(doc, "div")?.unchecked_into();
        style(&content, "display", "flex");
        style(&content, "flex-direction", "column");
        style(&content, "flex-grow", "1");
        // 中身が広くても、サイドバーを押し出さずに縮む。
        style(&content, "min-width", "0");
        mark_parent(
            &content,
            ParentLayout::Flex(naui_core::Orientation::Vertical),
        );

        append(&mount, &aside)?;
        append(&mount, &content)?;

        let this = Self(Rc::new(SidebarInner {
            document: doc.clone(),
            mount,
            aside,
            nav,
            content,
            sections: RefCell::new(Vec::new()),
            buttons: RefCell::new(Vec::new()),
            listeners: RefCell::new(Vec::new()),
            handler: SelectHandler::default(),
            selected: Cell::new(None),
            width: Cell::new(DEFAULT_SIDEBAR_WIDTH),
            id: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }));
        this.apply_width();
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
            append(&button, &icon_svg(doc, icon)?)?;
        }
        let label = create(doc, "span")?;
        label.set_text_content(Some(&item.label));
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
        self.0.width.set(width);
        self.apply_width();
    }

    /// サイドバーの幅。閉じていても開いたときの幅を返す。
    pub fn width(&self) -> f64 {
        self.0.width.get()
    }

    fn apply_width(&self) {
        style(&self.0.aside, "width", &format!("{}px", self.0.width.get()));
    }

    /// サイドバーを閉じる (`true`) か開く (`false`)。
    ///
    /// 閉じても項目と選択は残る。閉じている間は `hidden` で隠す。
    pub fn set_collapsed(&self, collapsed: bool) {
        self.0.aside.set_hidden(collapsed);
    }

    /// サイドバーが閉じているかどうか。
    pub fn is_collapsed(&self) -> bool {
        self.0.aside.hidden()
    }

    /// サイドバーの `<aside>`。バックエンド固有の脱出口。
    pub fn native_element(&self) -> Element {
        self.0.aside.clone().unchecked_into()
    }

    /// ウィンドウの中へ置く外枠 (サイドバーと中身の横並び)。
    pub(crate) fn mount(&self) -> HtmlElement {
        self.0.mount.clone()
    }

    /// 中身の側へウィンドウの子を置く。`None` なら空にする。
    pub(crate) fn set_content(&self, element: Option<&Element>) {
        self.0.content.set_inner_html("");
        if let Some(element) = element {
            if self.0.content.append_child(element).is_ok() {
                fill_parent(element);
                apply_child_layout(
                    element,
                    ParentLayout::Flex(naui_core::Orientation::Vertical),
                );
            }
        }
    }
}
