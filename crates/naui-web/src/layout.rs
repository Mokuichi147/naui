//! Size specifications and layout containers (Grid / Scroll / Spacer).
//!
//! The computing is done by the browser's CSS layout (Flexbox / Grid / scroll
//! areas); naui only writes properties.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{
    GridCell, Orientation, Padding, Result, ScrollMetrics, ScrollNotifier, ScrollPolicy,
    ScrollTarget, Sizing, Track,
};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, Element, HtmlElement, ResizeObserver, ScrollBehavior, ScrollToOptions};

use crate::to_error;
use crate::widgets::{create, impl_widget, Listener, Widget};

/// The attribute in which a parent container writes its own kind.
///
/// Which axis `flex-grow` and `align-self` affect depends on the parent's layout
/// direction. Letting children read the parent's kind gives the same result whether
/// `set_sizing` is called first or the child is added first.
const PARENT_ATTR: &str = "data-naui-parent";
/// The marker that the width is `Fill`.
const FILL_WIDTH_ATTR: &str = "data-naui-fill-width";
/// The marker that the height is `Fill`.
const FILL_HEIGHT_ATTR: &str = "data-naui-fill-height";

/// The kind of parent container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParentLayout {
    /// Flexbox. The value is the layout direction.
    Flex(Orientation),
    /// CSS Grid.
    Grid,
    /// Anything else (directly under a window, for example).
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

/// Tells the children what kind of container this is.
pub(crate) fn mark_parent(element: &HtmlElement, layout: ParentLayout) {
    let _ = element.set_attribute(PARENT_ATTR, layout.attribute());
}

/// Applies the size specification to the element.
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
    // `Fill` の画像と動画は、画素の大きさを自然な大きさとして親へ求めない。
    // 求めたままだと、縦の Scroll の中のように親が伸びられる所では、余りを受け取る
    // どころか画素の高さ (動画は幅と縦横比から決まる高さ) まで親を押し広げる。
    // 大きさの封じ込め (`contain: size`) で自然な大きさを 0 として扱わせる。
    // ただし縦横比も一緒に消えるので、`Auto` の軸 (画素の大きさや縦横比から
    // 決まる軸) が残るときは封じ込めない。
    // 上限付きの `Fill` の軸があるときも封じ込めない。上限が自然な大きさを抑える
    // ので親を押し広げず、封じ込めると Auto の行のように自然な大きさで決まる所で
    // 0 に潰れて見えなくなる (`contain-intrinsic-*` で上限を渡しても、Firefox では
    // 0 のままだった)。Auto の行では、画素の大きさを上限で抑えた大きさになる。
    if is_picture(element) {
        let has_auto = matches!(sizing.width, naui_core::Length::Auto)
            || matches!(sizing.height, naui_core::Length::Auto);
        let capped_fill = (sizing.width.is_fill() && sizing.max_width.is_some())
            || (sizing.height.is_fill() && sizing.max_height.is_some());
        if (sizing.width.is_fill() || sizing.height.is_fill()) && !has_auto && !capped_fill {
            let _ = style.set_property("contain", "size");
        } else {
            let _ = style.remove_property("contain");
        }
    }

    // 親が分かっていれば、その並び方向に合わせた指定もここで済ませる。
    layout_for_parent(element.unchecked_ref(), parent_layout(element));
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

/// Marks it as a child that fills its parent.
///
/// On other backends, the root directly under a window grows as the window's
/// content does (AppKit's contentView / a row of WinUI's Grid). So that the Web
/// behaves the same, the root gets `Fill` from the start.
///
/// Like `Fill` given with [`apply_sizing`], it may become smaller than its content
/// (`min-width` / `min-height: 0`), unless the app set a minimum. Otherwise the
/// Flexbox default minimum (the content's size) would keep a root holding a `Scroll`
/// as tall as everything in it, and the window, whose height follows the browser,
/// would cut it off instead of letting the `Scroll` scroll.
pub(crate) fn fill_parent(element: &Element) {
    let _ = element.set_attribute(FILL_WIDTH_ATTR, "");
    let _ = element.set_attribute(FILL_HEIGHT_ATTR, "");
    let style = element.unchecked_ref::<HtmlElement>().style();
    for property in ["min-width", "min-height"] {
        if style
            .get_property_value(property)
            .is_ok_and(|value| value.is_empty())
        {
            let _ = style.set_property(property, "0");
        }
    }
}

/// Prepares an element just put into a container.
///
/// Rewrites the settings that depend on the parent's layout direction (`flex-grow`
/// and so on), and tells the `Scroll`s inside that they were put back into the
/// document: the browser forgets the scroll position of an element taken out of it,
/// so a widget moved from another container would otherwise show its `Scroll`s from
/// the top.
pub(crate) fn apply_child_layout(element: &Element, parent: ParentLayout) {
    layout_for_parent(element, parent);
    tell_scrolls_reattached(element);
}

/// The attribute marking a `Scroll`'s element.
const SCROLL_ATTR: &str = "data-naui-scroll";
/// The event telling a `Scroll` that it was put into a container again.
const REATTACHED_EVENT: &str = "naui-reattached";

fn tell_scrolls_reattached(element: &Element) {
    let mut scrolls = Vec::new();
    if element.has_attribute(SCROLL_ATTR) {
        scrolls.push(element.clone());
    }
    if let Ok(found) = element.query_selector_all(&format!("[{SCROLL_ATTR}]")) {
        scrolls.extend(
            (0..found.length())
                .filter_map(|index| found.item(index))
                .filter_map(|node| node.dyn_into::<Element>().ok()),
        );
    }
    for scroll in scrolls {
        if let Ok(event) = web_sys::Event::new(REATTACHED_EVENT) {
            let _ = scroll.dispatch_event(&event);
        }
    }
}

/// Rewrites the settings that depend on the parent's layout direction
/// (`flex-grow` and so on).
///
/// Called when put into a container, when the size is specified again, and when a
/// `Stack` changes direction.
pub(crate) fn layout_for_parent(element: &Element, parent: ParentLayout) {
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
                stretch(&style, "align-self", "width");
            }
        }
        ParentLayout::Flex(Orientation::Horizontal) => {
            if fill_width {
                let _ = style.set_property("flex-grow", "1");
            }
            if fill_height {
                stretch(&style, "align-self", "height");
            }
        }
        ParentLayout::Grid => {
            // 上限で止まった `stretch` はマスの先頭に寄る。横はほかの環境
            // (GTK の Start、NSGridView の Leading、WinUI の包み) と同じなので
            // そのままでよい。
            if fill_width {
                stretch(&style, "justify-self", "width");
            }
            if fill_height {
                if style
                    .get_property_value("max-height")
                    .is_ok_and(|value| !value.is_empty())
                {
                    // 縦はほかの環境ではマスの中央に来るので、高さをマスの
                    // 100% (上限まで) にして中央に置く。グリッドの子の割合は
                    // マスの大きさに対して決まる。
                    let _ = style.set_property("align-self", "center");
                    let _ = style.set_property("height", "100%");
                } else {
                    stretch(&style, "align-self", "height");
                }
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

/// Whether it is an image or a video (`<img>` / `<video>`), a replaced element whose
/// natural size comes from its pixels.
fn is_picture(element: &HtmlElement) -> bool {
    let tag = element.tag_name();
    tag.eq_ignore_ascii_case("img") || tag.eq_ignore_ascii_case("video")
}

/// Stretches the element to the parent's size on one axis (`stretch`).
///
/// `stretch` only applies while that axis's size is `auto`. Some elements get a
/// size from the browser's own stylesheet (`<progress>` and `<select>`, for
/// example), and the inline style removed above does not undo that, so the size is
/// written as `auto` explicitly.
fn stretch(style: &web_sys::CssStyleDeclaration, align: &str, size: &str) {
    let _ = style.set_property(align, "stretch");
    let _ = style.set_property(size, "auto");
}

// ----------------------------------------------------------------- Spacer

struct SpacerInner {
    element: HtmlElement,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// A widget that is just empty space (`<div>`).
#[derive(Clone)]
pub struct Spacer(Rc<SpacerInner>);
impl_widget!(Spacer, element);

impl Spacer {
    pub(crate) fn new(document: &Document) -> Result<Self> {
        let element: HtmlElement = create(document, "div")?.unchecked_into();
        let this = Self(Rc::new(SpacerInner {
            element,
            file_drop: Default::default(),
        }));
        // 中身が無いので、余りをすべて受け取る。
        this.set_sizing(Sizing::fill());
        Ok(this)
    }
}

// ------------------------------------------------------------------- Grid

struct GridInner {
    element: HtmlElement,
    /// The placed children and their positions. Kept to remove them by cell.
    children: RefCell<Vec<(GridCell, Box<dyn Widget>)>>,
    columns: Cell<usize>,
    rows: Cell<usize>,
    column_tracks: RefCell<Vec<Track>>,
    row_tracks: RefCell<Vec<Track>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// A container that positions children by row and column (a CSS Grid `<div>`).
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
        // 横は先頭ぞろえ。CSS Grid の既定 (stretch) のままだと、中身に合わせる
        // はずの子までマスの幅いっぱいに伸びる。ほかの環境 (NSGridView の
        // Leading、WinUI の Left) と同じく、伸びるのは `Fill` の子
        // (justify-self: stretch) だけにする。
        let _ = element.style().set_property("justify-items", "start");
        mark_parent(&element, ParentLayout::Grid);
        Ok(Self(Rc::new(GridInner {
            element,
            children: RefCell::new(Vec::new()),
            columns: Cell::new(0),
            rows: Cell::new(0),
            column_tracks: RefCell::new(Vec::new()),
            row_tracks: RefCell::new(Vec::new()),
            file_drop: Default::default(),
        })))
    }

    /// The spacing between columns and between rows.
    pub fn set_spacing(&self, column: f64, row: f64) {
        let style = self.0.element.style();
        let _ = style.set_property("column-gap", &format!("{column}px"));
        let _ = style.set_property("row-gap", &format!("{row}px"));
    }

    /// The outer padding.
    pub fn set_padding(&self, padding: Padding) {
        let _ = self.0.element.style().set_property(
            "padding",
            &format!(
                "{}px {}px {}px {}px",
                padding.top, padding.right, padding.bottom, padding.left
            ),
        );
    }

    /// Places a child at the given position. Missing rows and columns are added
    /// automatically.
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

    /// Replaces what is in the cell. Whatever was in the same cell is removed.
    pub fn replace(&self, child: &dyn Widget, cell: GridCell) {
        self.remove(cell);
        self.attach(child, cell);
    }

    /// Removes whatever is in the given cell. Does nothing if it is empty.
    ///
    /// Only the column and row of `cell` are considered, not the span.
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

    /// Removes every child. The row and column settings stay.
    pub fn clear(&self) {
        for (_, child) in std::mem::take(&mut *self.0.children.borrow_mut()) {
            let _ = self.0.element.remove_child(&child.native_element());
        }
    }

    /// How a column's width is determined.
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

    /// How a row's height is determined.
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

    /// The current number of columns.
    pub fn columns(&self) -> usize {
        self.0.columns.get()
    }

    /// The current number of rows.
    pub fn rows(&self) -> usize {
        self.0.rows.get()
    }

    /// The number of children placed.
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
    /// Does not notify for `scroll` events to the same position.
    on_scroll: ScrollNotifier,
    /// The destination requested before the size was decided (before it is in the
    /// document, inside a hidden tab, and so on). Applied once it has a size.
    pending: Cell<Option<ScrollTarget>>,
    /// The position while it was last shown. The browser forgets the position of an
    /// element taken out of the document, so it is scrolled back here once it is put
    /// into a container again.
    shown_offset: Cell<(f64, f64)>,
    listener: RefCell<Option<Listener>>,
    reattached_listener: RefCell<Option<Listener>>,
    /// The subscription to size changes. Dropping it also unsubscribes.
    observer: RefCell<Option<ResizeObserver>>,
    observer_callback: RefCell<Option<Closure<dyn FnMut(JsValue)>>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

impl Drop for ScrollInner {
    fn drop(&mut self) {
        if let Some(observer) = self.observer.borrow_mut().take() {
            observer.disconnect();
        }
    }
}

/// A container that scrolls when its contents overflow (a `<div>` with
/// `overflow`).
#[derive(Clone)]
pub struct Scroll(Rc<ScrollInner>);
impl_widget!(Scroll, element);

impl Scroll {
    pub(crate) fn new(document: &Document) -> Result<Self> {
        let element: HtmlElement = create(document, "div")?.unchecked_into();
        // 中身を表示領域の大きさに合わせるため、1 マスの Grid にする (set_policy)。
        let _ = element.style().set_property("display", "grid");
        let _ = element.set_attribute(SCROLL_ATTR, "");
        let this = Self(Rc::new(ScrollInner {
            element,
            child: RefCell::new(None),
            on_scroll: ScrollNotifier::default(),
            pending: Cell::new(None),
            shown_offset: Cell::new((0.0, 0.0)),
            listener: RefCell::new(None),
            reattached_listener: RefCell::new(None),
            observer: RefCell::new(None),
            observer_callback: RefCell::new(None),
            file_drop: Default::default(),
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
        let weak = Rc::downgrade(&self.0);
        let listener = Listener::attach(self.0.element.as_ref(), REATTACHED_EVENT, move || {
            if let Some(inner) = weak.upgrade() {
                Scroll(inner).restore_shown_offset();
            }
        })?;
        *self.0.reattached_listener.borrow_mut() = Some(listener);

        // 文書に載ったとき・隠れていたタブが出たときにも呼ばれる。
        let weak = Rc::downgrade(&self.0);
        let callback = Closure::<dyn FnMut(JsValue)>::new(move |_entries: JsValue| {
            if let Some(inner) = weak.upgrade() {
                Scroll(inner).apply_pending();
            }
        });
        let observer = ResizeObserver::new(callback.as_ref().unchecked_ref())
            .map_err(|e| to_error("create the ResizeObserver", e))?;
        observer.observe(self.0.element.as_ref());
        *self.0.observer.borrow_mut() = Some(observer);
        *self.0.observer_callback.borrow_mut() = Some(callback);
        Ok(())
    }

    /// Whether scrolling is allowed horizontally and vertically. The default is
    /// horizontal `Never`, vertical `Auto`.
    pub fn set_policy(&self, horizontal: ScrollPolicy, vertical: ScrollPolicy) {
        let style = self.0.element.style();
        let _ = style.set_property("overflow-x", overflow(horizontal));
        let _ = style.set_property("overflow-y", overflow(vertical));
        let _ = style.set_property("grid-template-columns", content_track(horizontal));
        let _ = style.set_property("grid-template-rows", content_track(vertical));
    }

    /// The content to scroll. Each call replaces it.
    pub fn set_child(&self, child: &dyn Widget) {
        self.0.element.set_inner_html("");
        let element = child.native_element();
        if self.0.element.append_child(&element).is_ok() {
            apply_child_layout(&element, ParentLayout::Block);
            *self.0.child.borrow_mut() = Some(child.boxed_clone());
        }
    }

    /// The current scroll position and sizes.
    ///
    /// The browser finishes layout when they are read, so they can be measured with the
    /// new sizes even right after the contents change.
    pub fn metrics(&self) -> ScrollMetrics {
        let element = &self.0.element;
        let viewport_width = element.client_width() as f64;
        let viewport_height = element.client_height() as f64;
        let mut metrics = ScrollMetrics {
            x: scroll_offset(element, "scrollLeft"),
            y: scroll_offset(element, "scrollTop"),
            viewport_width,
            viewport_height,
            content_width: (element.scroll_width() as f64).max(viewport_width),
            content_height: (element.scroll_height() as f64).max(viewport_height),
        };
        // 大きさ (`clientHeight` / `scrollHeight`) は整数に丸められ、位置は
        // 拡大率によって小数になる。末尾まで 1 px 未満の差は丸めによるもの
        // なので、末尾にいるとみなす。
        metrics.x = snap_to_end(metrics.x, metrics.max_x());
        metrics.y = snap_to_end(metrics.y, metrics.max_y());
        metrics
    }

    /// Scrolls to the given position, clamped to the scrollable range.
    ///
    /// If the size is not decided yet (before it is in the document, for example), it
    /// scrolls once it is.
    pub fn scroll_to(&self, x: f64, y: f64) {
        self.request(ScrollTarget::To { x, y });
    }

    /// Scrolls to the vertical end (the very bottom). The horizontal position stays.
    pub fn scroll_to_end(&self) {
        self.request(ScrollTarget::End);
    }

    /// Notified when the scroll position changes.
    ///
    /// Besides user interaction, it also arrives for moves caused by `scroll_to` or by
    /// the contents shrinking. It does not arrive when only the sizes change. It is
    /// delivered in response to the browser's `scroll` event, so it comes after
    /// `scroll_to` returns.
    pub fn on_scroll(&self, f: impl FnMut(ScrollMetrics) + 'static) {
        self.0.on_scroll.set(&self.metrics(), f);
    }

    fn request(&self, target: ScrollTarget) {
        self.0.pending.set(Some(target));
        self.apply_pending();
    }

    /// Scrolls to the remembered destination. If the size is not decided, keeps
    /// remembering it.
    fn apply_pending(&self) {
        let Some(target) = self.0.pending.get() else {
            return;
        };
        let metrics = self.metrics();
        if !has_viewport(&metrics) {
            return;
        }
        self.0.pending.set(None);
        let (x, y) = target.resolve(&metrics);
        self.0.shown_offset.set((x, y));
        if (x, y) != (metrics.x, metrics.y) {
            // ページの CSS に `scroll-behavior: smooth` があっても、その場で送る。
            let options = ScrollToOptions::new();
            options.set_left(x);
            options.set_top(y);
            options.set_behavior(ScrollBehavior::Instant);
            self.0.element.scroll_to_with_scroll_to_options(&options);
        }
    }

    fn notify_if_moved(&self) {
        let metrics = self.metrics();
        if has_viewport(&metrics) {
            self.0.shown_offset.set((metrics.x, metrics.y));
        }
        self.0.on_scroll.notify_if_moved(|| self.metrics());
    }

    /// Scrolls back to the position from while it was shown, after being put into a
    /// container again ([`apply_child_layout`]).
    ///
    /// The browser drops the scroll position of an element taken out of the document
    /// and shows it again from the top, while the other platforms keep it. Taking it
    /// out and putting it back within one task shows no size change, so
    /// `ResizeObserver` cannot tell; the container does. If it has no size yet, the
    /// position waits like a `scroll_to` before it is shown.
    ///
    /// A destination already waiting is applied too. Put into a container that is
    /// not in the document yet, the position waits; when that container is then put
    /// into the document with the same size, `ResizeObserver` stays silent again, so
    /// this is the only chance to apply it.
    fn restore_shown_offset(&self) {
        if self.0.pending.get().is_none() {
            let (x, y) = self.0.shown_offset.get();
            let metrics = self.metrics();
            if (metrics.x, metrics.y) == (x, y) {
                return;
            }
            self.0.pending.set(Some(ScrollTarget::To { x, y }));
        }
        self.apply_pending();
    }
}

/// Whether it has a visible area (it is in the document and shown).
fn has_viewport(metrics: &ScrollMetrics) -> bool {
    metrics.viewport_width > 0.0 && metrics.viewport_height > 0.0
}

/// Reads `scrollTop` / `scrollLeft` as fractions. web-sys's `scroll_top()` returns
/// an integer, so at some zoom levels it would look as if the end had not been
/// reached.
fn scroll_offset(element: &HtmlElement, name: &str) -> f64 {
    js_sys::Reflect::get(element, &JsValue::from_str(name))
        .ok()
        .and_then(|value| value.as_f64())
        .unwrap_or(0.0)
}

/// Snaps to the end if less than 1 px from it.
fn snap_to_end(offset: f64, max: f64) -> f64 {
    if (max - offset).abs() < 1.0 {
        max
    } else {
        offset
    }
}

/// The size of the single cell that holds the content.
///
/// Like the other backends (GtkViewport, WinUI's ScrollViewer, the clip-view
/// constraints on macOS), the content is never smaller than the visible area: it
/// matches it in a direction that does not scroll, and grows past it in one that
/// does. So a `Fill` child inside takes the leftover space while the content fits.
///
/// An `auto` track takes the content's size and is then stretched to the visible
/// area (the default `align-content` / `justify-content`). `minmax(100%,
/// max-content)` would not do: a track only grows past its minimum into free space,
/// so tall content would stay at the visible area. A percentage against an
/// undecided size counts as `auto`, so a `Scroll` without a height still follows
/// its content.
fn content_track(policy: ScrollPolicy) -> &'static str {
    if policy.is_enabled() {
        "auto"
    } else {
        "100%"
    }
}

fn overflow(policy: ScrollPolicy) -> &'static str {
    match policy {
        ScrollPolicy::Auto => "auto",
        ScrollPolicy::Always => "scroll",
        ScrollPolicy::Never => "hidden",
    }
}
