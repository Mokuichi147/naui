//! 大きさの指定と、レイアウト用のコンテナ (Grid / Scroll / Spacer)。
//!
//! 計算するのはブラウザの CSS レイアウト (Flexbox / Grid / スクロール領域) で、
//! miui 側はプロパティを書くだけ。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use miui_core::{GridCell, Orientation, Padding, Result, ScrollPolicy, Sizing, Track};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement};

use crate::widgets::{create, impl_widget, Widget};

/// 親コンテナが自分の種類を書いておく属性。
///
/// `flex-grow` と `align-self` はどちらの軸に効くかが親の並び方向で変わる。
/// 子から親の種類を読めるようにしておくと、`set_sizing` を先に呼んでも
/// 後から追加しても同じ結果になる。
const PARENT_ATTR: &str = "data-miui-parent";
/// 幅が `Fill` であることの目印。
const FILL_WIDTH_ATTR: &str = "data-miui-fill-width";
/// 高さが `Fill` であることの目印。
const FILL_HEIGHT_ATTR: &str = "data-miui-fill-height";

/// 親コンテナの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParentLayout {
    /// Flexbox。値は並ぶ向き。
    Flex(Orientation),
    /// CSS Grid。
    Grid,
    /// それ以外 (ウィンドウ直下など)。
    Block,
}

impl ParentLayout {
    fn attribute(self) -> &'static str {
        match self {
            ParentLayout::Flex(Orientation::Vertical) => "flex-column",
            ParentLayout::Flex(Orientation::Horizontal) => "flex-row",
            ParentLayout::Grid => "grid",
            ParentLayout::Block => "block",
        }
    }

    fn from_attribute(value: &str) -> Self {
        match value {
            "flex-column" => ParentLayout::Flex(Orientation::Vertical),
            "flex-row" => ParentLayout::Flex(Orientation::Horizontal),
            "grid" => ParentLayout::Grid,
            _ => ParentLayout::Block,
        }
    }
}

/// 自分が何のコンテナかを子に伝える。
pub(crate) fn mark_parent(element: &HtmlElement, layout: ParentLayout) {
    let _ = element.set_attribute(PARENT_ATTR, layout.attribute());
}

/// 大きさの指定を要素へ反映する。
pub(crate) fn apply_sizing(element: &Element, sizing: Sizing) {
    let element: &HtmlElement = element.unchecked_ref();
    let style = element.style();
    // 幅や高さを指定したときに、余白で膨らまないようにする。
    let _ = style.set_property("box-sizing", "border-box");

    set_length(element, true, sizing.width);
    set_length(element, false, sizing.height);
    set_limit(element, "min-width", sizing.min_width);
    set_limit(element, "max-width", sizing.max_width);
    set_limit(element, "min-height", sizing.min_height);
    set_limit(element, "max-height", sizing.max_height);

    // 親が分かっていれば、その並び方向に合わせた指定もここで済ませる。
    apply_child_layout(element.unchecked_ref(), parent_layout(element));
}

fn set_length(element: &HtmlElement, horizontal: bool, length: miui_core::Length) {
    let style = element.style();
    let (property, attribute) = if horizontal {
        ("width", FILL_WIDTH_ATTR)
    } else {
        ("height", FILL_HEIGHT_ATTR)
    };
    match length {
        miui_core::Length::Auto => {
            let _ = style.remove_property(property);
            let _ = element.remove_attribute(attribute);
        }
        miui_core::Length::Fixed(value) => {
            let _ = style.set_property(property, &format!("{value}px"));
            // 固定と言った以上、Flexbox でも縮ませない。
            let _ = style.set_property("flex-shrink", "0");
            let _ = element.remove_attribute(attribute);
        }
        miui_core::Length::Fill => {
            let _ = style.remove_property(property);
            let _ = element.set_attribute(attribute, "");
        }
    }
}

fn set_limit(element: &HtmlElement, property: &str, value: Option<f64>) {
    let style = element.style();
    match value {
        Some(value) => {
            let _ = style.set_property(property, &format!("{value}px"));
        }
        None => {
            let _ = style.remove_property(property);
        }
    }
}

fn parent_layout(element: &HtmlElement) -> ParentLayout {
    element
        .parent_element()
        .and_then(|parent| parent.get_attribute(PARENT_ATTR))
        .map(|value| ParentLayout::from_attribute(&value))
        .unwrap_or(ParentLayout::Block)
}

/// 親の並び方向に依存する指定 (`flex-grow` など) を書き直す。
///
/// コンテナへ入れたときと、大きさを指定し直したときの両方から呼ぶ。
pub(crate) fn apply_child_layout(element: &Element, parent: ParentLayout) {
    let element: &HtmlElement = element.unchecked_ref();
    let style = element.style();
    let fill_width = element.has_attribute(FILL_WIDTH_ATTR);
    let fill_height = element.has_attribute(FILL_HEIGHT_ATTR);

    let _ = style.remove_property("flex-grow");
    let _ = style.remove_property("align-self");
    let _ = style.remove_property("justify-self");

    match parent {
        // 主軸は flex-grow で余りを受け取り、交差軸は stretch で親に合わせる。
        ParentLayout::Flex(Orientation::Vertical) => {
            if fill_height {
                let _ = style.set_property("flex-grow", "1");
            }
            if fill_width {
                let _ = style.set_property("align-self", "stretch");
            }
        }
        ParentLayout::Flex(Orientation::Horizontal) => {
            if fill_width {
                let _ = style.set_property("flex-grow", "1");
            }
            if fill_height {
                let _ = style.set_property("align-self", "stretch");
            }
        }
        ParentLayout::Grid => {
            if fill_width {
                let _ = style.set_property("justify-self", "stretch");
            }
            if fill_height {
                let _ = style.set_property("align-self", "stretch");
            }
        }
        ParentLayout::Block => {
            if fill_width {
                let _ = style.set_property("width", "100%");
            }
            if fill_height {
                let _ = style.set_property("height", "100%");
            }
        }
    }
}

// ----------------------------------------------------------------- Spacer

struct SpacerInner {
    element: HtmlElement,
}

/// 余白そのものになるウィジェット (`<div>`)。
#[derive(Clone)]
pub struct Spacer(Rc<SpacerInner>);
impl_widget!(Spacer, element);

impl Spacer {
    pub(crate) fn new(document: &Document) -> Result<Self> {
        let element: HtmlElement = create(document, "div")?.unchecked_into();
        let this = Self(Rc::new(SpacerInner { element }));
        // 中身が無いので、余りをすべて受け取る。
        this.set_sizing(Sizing::fill());
        Ok(this)
    }
}

// ------------------------------------------------------------------- Grid

struct GridInner {
    element: HtmlElement,
    children: RefCell<Vec<Box<dyn Widget>>>,
    columns: Cell<usize>,
    rows: Cell<usize>,
    column_tracks: RefCell<Vec<Track>>,
    row_tracks: RefCell<Vec<Track>>,
}

/// 行と列で位置を決めるコンテナ (CSS Grid の `<div>`)。
#[derive(Clone)]
pub struct Grid(Rc<GridInner>);
impl_widget!(Grid, element);

impl Grid {
    pub(crate) fn new(document: &Document) -> Result<Self> {
        let element: HtmlElement = create(document, "div")?.unchecked_into();
        let _ = element.style().set_property("display", "grid");
        mark_parent(&element, ParentLayout::Grid);
        Ok(Self(Rc::new(GridInner {
            element,
            children: RefCell::new(Vec::new()),
            columns: Cell::new(0),
            rows: Cell::new(0),
            column_tracks: RefCell::new(Vec::new()),
            row_tracks: RefCell::new(Vec::new()),
        })))
    }

    /// 列間・行間のすき間。
    pub fn set_spacing(&self, column: f64, row: f64) {
        let style = self.0.element.style();
        let _ = style.set_property("column-gap", &format!("{column}px"));
        let _ = style.set_property("row-gap", &format!("{row}px"));
    }

    /// 外周の余白。
    pub fn set_padding(&self, padding: Padding) {
        let _ = self.0.element.style().set_property(
            "padding",
            &format!(
                "{}px {}px {}px {}px",
                padding.top, padding.right, padding.bottom, padding.left
            ),
        );
    }

    /// 指定した場所に子を置く。足りない行と列は自動で足される。
    pub fn attach(&self, child: &dyn Widget, cell: GridCell) {
        let element = child.native_element();
        if self.0.element.append_child(&element).is_err() {
            return;
        }
        let style: &HtmlElement = element.unchecked_ref();
        let style = style.style();
        let _ = style.set_property(
            "grid-column",
            &format!("{} / span {}", cell.column + 1, cell.column_span),
        );
        let _ = style.set_property(
            "grid-row",
            &format!("{} / span {}", cell.row + 1, cell.row_span),
        );
        apply_child_layout(&element, ParentLayout::Grid);

        self.grow_to(cell.columns_needed(), cell.rows_needed());
        self.0.children.borrow_mut().push(child.boxed_clone());
    }

    /// 列の幅の決め方。
    pub fn set_column_track(&self, index: usize, track: Track) {
        {
            let mut tracks = self.0.column_tracks.borrow_mut();
            if tracks.len() <= index {
                tracks.resize(index + 1, Track::Auto);
            }
            tracks[index] = track;
        }
        self.grow_to(index + 1, 0);
        self.apply_tracks();
    }

    /// 行の高さの決め方。
    pub fn set_row_track(&self, index: usize, track: Track) {
        {
            let mut tracks = self.0.row_tracks.borrow_mut();
            if tracks.len() <= index {
                tracks.resize(index + 1, Track::Auto);
            }
            tracks[index] = track;
        }
        self.grow_to(0, index + 1);
        self.apply_tracks();
    }

    /// いまある列数。
    pub fn columns(&self) -> usize {
        self.0.columns.get()
    }

    /// いまある行数。
    pub fn rows(&self) -> usize {
        self.0.rows.get()
    }

    /// 置いた子の数。
    pub fn len(&self) -> usize {
        self.0.children.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn grow_to(&self, columns: usize, rows: usize) {
        let changed = columns > self.0.columns.get() || rows > self.0.rows.get();
        self.0.columns.set(self.0.columns.get().max(columns));
        self.0.rows.set(self.0.rows.get().max(rows));
        if changed {
            self.apply_tracks();
        }
    }

    fn apply_tracks(&self) {
        let style = self.0.element.style();
        let _ = style.set_property(
            "grid-template-columns",
            &template(self.0.columns.get(), &self.0.column_tracks.borrow()),
        );
        let _ = style.set_property(
            "grid-template-rows",
            &template(self.0.rows.get(), &self.0.row_tracks.borrow()),
        );
    }
}

fn template(count: usize, tracks: &[Track]) -> String {
    (0..count)
        .map(|index| match tracks.get(index).copied().unwrap_or_default() {
            Track::Auto => "auto".to_string(),
            Track::Fixed(value) => format!("{value}px"),
            track @ Track::Fill(_) => format!("{}fr", track.weight()),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ----------------------------------------------------------------- Scroll

struct ScrollInner {
    element: HtmlElement,
    child: RefCell<Option<Box<dyn Widget>>>,
}

/// 中身がはみ出したらスクロールさせるコンテナ (`overflow` を付けた `<div>`)。
#[derive(Clone)]
pub struct Scroll(Rc<ScrollInner>);
impl_widget!(Scroll, element);

impl Scroll {
    pub(crate) fn new(document: &Document) -> Result<Self> {
        let element: HtmlElement = create(document, "div")?.unchecked_into();
        let this = Self(Rc::new(ScrollInner {
            element,
            child: RefCell::new(None),
        }));
        this.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
        Ok(this)
    }

    /// 横 / 縦それぞれのスクロールの許可。既定は横 `Never`・縦 `Auto`。
    pub fn set_policy(&self, horizontal: ScrollPolicy, vertical: ScrollPolicy) {
        let style = self.0.element.style();
        let _ = style.set_property("overflow-x", overflow(horizontal));
        let _ = style.set_property("overflow-y", overflow(vertical));
    }

    /// スクロールさせる中身。呼ぶたびに置き換わる。
    pub fn set_child(&self, child: &dyn Widget) {
        self.0.element.set_inner_html("");
        let element = child.native_element();
        if self.0.element.append_child(&element).is_ok() {
            apply_child_layout(&element, ParentLayout::Block);
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
        }
    }
}

fn overflow(policy: ScrollPolicy) -> &'static str {
    match policy {
        ScrollPolicy::Auto => "auto",
        ScrollPolicy::Always => "scroll",
        ScrollPolicy::Never => "hidden",
    }
}
