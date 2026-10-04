//! 大きさの指定と、レイアウト用のコンテナ (Grid / Scroll / Spacer)。
//!
//! 計算するのはブラウザの CSS レイアウト (Flexbox / Grid / スクロール領域) で、
//! naui 側はプロパティを書くだけ。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{
    GridCell, Orientation, Padding, Result, ScrollMetrics, ScrollPolicy, ScrollTarget, Sizing,
    Track,
};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, Element, HtmlElement, ResizeObserver};

use crate::to_error;
use crate::widgets::{create, impl_widget, Listener, ValueHandler, Widget};

/// 親コンテナが自分の種類を書いておく属性。
///
/// `flex-grow` と `align-self` はどちらの軸に効くかが親の並び方向で変わる。
/// 子から親の種類を読めるようにしておくと、`set_sizing` を先に呼んでも
/// 後から追加しても同じ結果になる。
const PARENT_ATTR: &str = "data-naui-parent";
/// 幅が `Fill` であることの目印。
const FILL_WIDTH_ATTR: &str = "data-naui-fill-width";
/// 高さが `Fill` であることの目印。
const FILL_HEIGHT_ATTR: &str = "data-naui-fill-height";

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
    // 固定長のときだけ set_length が付け直す。
    let _ = style.remove_property("flex-shrink");

    set_length(element, true, sizing.width);
    set_length(element, false, sizing.height);
    set_limit(element, "min-width", sizing.min_width);
    set_limit(element, "max-width", sizing.max_width);
    set_limit(element, "min-height", sizing.min_height);
    set_limit(element, "max-height", sizing.max_height);

    // Flex/Grid の既定の min-size は中身の intrinsic size になるため、
    // Fill の画像や動画が自然サイズを親の最小幅にしてしまわないようにする。
    if sizing.width.is_fill() && sizing.min_width.is_none() {
        let _ = style.set_property("min-width", "0");
    }
    if sizing.height.is_fill() && sizing.min_height.is_none() {
        let _ = style.set_property("min-height", "0");
    }

    // 親が分かっていれば、その並び方向に合わせた指定もここで済ませる。
    apply_child_layout(element.unchecked_ref(), parent_layout(element));
}

fn set_length(element: &HtmlElement, horizontal: bool, length: naui_core::Length) {
    let style = element.style();
    let (property, attribute) = if horizontal {
        ("width", FILL_WIDTH_ATTR)
    } else {
        ("height", FILL_HEIGHT_ATTR)
    };
    match length {
        naui_core::Length::Auto => {
            let _ = style.remove_property(property);
            let _ = element.remove_attribute(attribute);
        }
        naui_core::Length::Fixed(value) => {
            let _ = style.set_property(property, &format!("{value}px"));
            // 固定と言った以上、Flexbox でも縮ませない。
            let _ = style.set_property("flex-shrink", "0");
            let _ = element.remove_attribute(attribute);
        }
        naui_core::Length::Fill => {
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

/// 親いっぱいに広がる子として印を付ける。
///
/// ウィンドウ直下のルートは、他のバックエンドではウィンドウの中身
/// (AppKit の contentView / WinUI の Grid の行) がそのまま広がる。
/// Web でも同じになるよう、ルートには最初から `Fill` を入れておく。
pub(crate) fn fill_parent(element: &Element) {
    let _ = element.set_attribute(FILL_WIDTH_ATTR, "");
    let _ = element.set_attribute(FILL_HEIGHT_ATTR, "");
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
    // `Fill` の指定は幅・高さのプロパティを使わないので、
    // 前の親向けに書いた `100%` が残らないようにする。
    if fill_width {
        let _ = style.remove_property("width");
    }
    if fill_height {
        let _ = style.remove_property("height");
    }

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
    /// 置いた子と、その置き場所。マス単位で外すために持つ。
    children: RefCell<Vec<(GridCell, Box<dyn Widget>)>>,
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
        // 縦は中央ぞろえ。高さの違うもの (ラベルと入力欄など) を同じ行に置いても
        // 上端で揃わないようにする。`Fill` の子は align-self: stretch で上書きされる。
        let _ = element.style().set_property("align-items", "center");
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
        self.0
            .children
            .borrow_mut()
            .push((cell, child.boxed_clone()));
    }

    /// そのマスの中身を差し替える。同じマスに置かれていたものは外れる。
    pub fn replace(&self, child: &dyn Widget, cell: GridCell) {
        self.remove(cell);
        self.attach(child, cell);
    }

    /// 指定したマスに置かれているものを外す。何も無ければ何もしない。
    ///
    /// 見るのは `cell` の列と行だけで、span は見ない。
    pub fn remove(&self, cell: GridCell) {
        let mut children = self.0.children.borrow_mut();
        let mut index = 0;
        while index < children.len() {
            if children[index].0.column == cell.column && children[index].0.row == cell.row {
                let (_, child) = children.remove(index);
                let _ = self.0.element.remove_child(&child.native_element());
            } else {
                index += 1;
            }
        }
    }

    /// 子をすべて外す。行と列の指定はそのまま残る。
    pub fn clear(&self) {
        for (_, child) in std::mem::take(&mut *self.0.children.borrow_mut()) {
            let _ = self.0.element.remove_child(&child.native_element());
        }
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
        .map(
            |index| match tracks.get(index).copied().unwrap_or_default() {
                Track::Auto => "auto".to_string(),
                Track::Fixed(value) => format!("{value}px"),
                track @ Track::Fill(_) => format!("{}fr", track.weight()),
            },
        )
        .collect::<Vec<_>>()
        .join(" ")
}

// ----------------------------------------------------------------- Scroll

struct ScrollInner {
    element: HtmlElement,
    child: RefCell<Option<Box<dyn Widget>>>,
    on_scroll: ValueHandler<ScrollMetrics>,
    /// 最後に通知した位置。同じ位置への `scroll` イベントでは通知しない。
    last_offset: Cell<(f64, f64)>,
    /// 大きさが決まる前 (文書に載る前・隠れたタブの中など) に頼まれた
    /// 行き先。大きさが付いた時点で適用する。
    pending: Cell<Option<ScrollTarget>>,
    listener: RefCell<Option<Listener>>,
    /// 大きさの変化の購読。落とすと購読も外れる。
    observer: RefCell<Option<ResizeObserver>>,
    observer_callback: RefCell<Option<Closure<dyn FnMut(JsValue)>>>,
}

impl Drop for ScrollInner {
    fn drop(&mut self) {
        if let Some(observer) = self.observer.borrow_mut().take() {
            observer.disconnect();
        }
    }
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
            on_scroll: ValueHandler::default(),
            last_offset: Cell::new((0.0, 0.0)),
            pending: Cell::new(None),
            listener: RefCell::new(None),
            observer: RefCell::new(None),
            observer_callback: RefCell::new(None),
        }));
        this.set_policy(ScrollPolicy::Never, ScrollPolicy::Auto);
        this.observe()?;
        Ok(this)
    }

    fn observe(&self) -> Result<()> {
        let weak = Rc::downgrade(&self.0);
        let listener = Listener::attach(self.0.element.as_ref(), "scroll", move || {
            if let Some(inner) = weak.upgrade() {
                Scroll(inner).notify_if_moved();
            }
        })?;
        *self.0.listener.borrow_mut() = Some(listener);

        // 文書に載ったとき・隠れていたタブが出たときにも呼ばれる。
        let weak = Rc::downgrade(&self.0);
        let callback = Closure::<dyn FnMut(JsValue)>::new(move |_entries: JsValue| {
            if let Some(inner) = weak.upgrade() {
                Scroll(inner).apply_pending();
            }
        });
        let observer = ResizeObserver::new(callback.as_ref().unchecked_ref())
            .map_err(|e| to_error("ResizeObserver の生成", e))?;
        observer.observe(self.0.element.as_ref());
        *self.0.observer.borrow_mut() = Some(observer);
        *self.0.observer_callback.borrow_mut() = Some(callback);
        Ok(())
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

    /// いまのスクロール位置と大きさ。
    ///
    /// 読むときにブラウザがレイアウトを済ませるので、直前に中身を
    /// 変えていても新しい大きさで測れる。
    pub fn metrics(&self) -> ScrollMetrics {
        let element = &self.0.element;
        let viewport_width = element.client_width() as f64;
        let viewport_height = element.client_height() as f64;
        ScrollMetrics {
            x: element.scroll_left() as f64,
            y: element.scroll_top() as f64,
            viewport_width,
            viewport_height,
            content_width: (element.scroll_width() as f64).max(viewport_width),
            content_height: (element.scroll_height() as f64).max(viewport_height),
        }
    }

    /// 指定した位置へ送る。送れる範囲に丸める。
    ///
    /// まだ大きさが決まっていない (文書に載る前など) ときは、決まった時点で送る。
    pub fn scroll_to(&self, x: f64, y: f64) {
        self.request(ScrollTarget::To { x, y });
    }

    /// 縦の末尾 (いちばん下) へ送る。横の位置はそのまま。
    pub fn scroll_to_end(&self) {
        self.request(ScrollTarget::End);
    }

    /// スクロール位置が変わったときの通知。
    ///
    /// 利用者の操作に加え、`scroll_to` や中身が縮んだことによる移動でも
    /// 届く。大きさだけが変わったときは届かない。ブラウザの `scroll`
    /// イベントを受けて届くので、`scroll_to` から戻った後になる。
    pub fn on_scroll(&self, f: impl FnMut(ScrollMetrics) + 'static) {
        self.0.last_offset.set(offset_of(&self.metrics()));
        self.0.on_scroll.set(f);
    }

    fn request(&self, target: ScrollTarget) {
        self.0.pending.set(Some(target));
        self.apply_pending();
    }

    /// 覚えている行き先へ送る。大きさが決まっていなければ、まだ覚えておく。
    fn apply_pending(&self) {
        let Some(target) = self.0.pending.get() else {
            return;
        };
        let metrics = self.metrics();
        if metrics.viewport_width <= 0.0 || metrics.viewport_height <= 0.0 {
            return;
        }
        self.0.pending.set(None);
        let (x, y) = target.resolve(&metrics);
        if (x, y) != (metrics.x, metrics.y) {
            self.0.element.scroll_to_with_x_and_y(x, y);
        }
    }

    fn notify_if_moved(&self) {
        let metrics = self.metrics();
        let offset = offset_of(&metrics);
        if offset == self.0.last_offset.replace(offset) {
            return;
        }
        self.0.on_scroll.emit(metrics);
    }
}

fn offset_of(metrics: &ScrollMetrics) -> (f64, f64) {
    (metrics.x, metrics.y)
}

fn overflow(policy: ScrollPolicy) -> &'static str {
    match policy {
        ScrollPolicy::Auto => "auto",
        ScrollPolicy::Always => "scroll",
        ScrollPolicy::Never => "hidden",
    }
}
