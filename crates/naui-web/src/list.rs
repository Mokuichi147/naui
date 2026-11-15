//! Lists (DOM).
//!
//! **The structure depends on the rows' contents.**
//!
//! | Rows | Built from |
//! | --- | --- |
//! | Text only, up to [`ROW_WINDOW_THRESHOLD`] rows | `<select size>` + `<option>` — the browser's standard list box itself |
//! | With `detail` / custom content, or more rows than that | `<ul role="listbox">` + `<li role="option">` — composed |
//!
//! The content model of `<option>` is **text only**; it can hold neither elements
//! nor line breaks. Showing two-line rows means leaving `<select>`. Still,
//! `<select>` is a real control whose selection, keyboard navigation and scrolling
//! the browser takes care of, so **it keeps being used when that is not needed
//! (text-only rows)**.
//!
//! The composed one also uses the browser's system colors for the frame and the
//! selection (`Field` / `SelectedItem` / `Highlight` / `GrayText`) as is; naui
//! decides no colors.
//!
//! **With many rows, only the rows on screen are built** (the same mechanism as
//! [`crate::Table`]). A `<select>` creates every `<option>` it is given and has no place
//! for spacers, so such lists are always composed: `<li>`s exist only for the window,
//! and spacer `<li>`s carry the height of the rest.
//!
//! Row activation (`dblclick` and Enter) and right-clicks (`contextmenu`) are
//! received together on the outer frame, which stays the same even when the
//! contents are swapped, and the row is looked up from the clicked element.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{
    keeps_row_window, list_row_window, Align, ListItem, Orientation, Point, Result, RowSource,
    RowWindow, SelectionMode, Sizing, TextColor, TextStyle, ROW_WINDOW_OVERSCAN,
    ROW_WINDOW_THRESHOLD,
};
use wasm_bindgen::prelude::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    Document, Element, Event, HtmlElement, HtmlOptionElement, HtmlSelectElement, KeyboardEvent,
    MouseEvent, ResizeObserver,
};

use crate::widgets::{create, impl_widget, Label, Listener, Stack, ValueHandler, Widget};

/// The handler notified when a row is clicked.
///
/// Replacing the same row's callback during the call does not double-borrow.
#[derive(Clone, Default)]
pub(crate) struct ActivationHandler(Rc<RefCell<Option<Box<dyn FnMut()>>>>);

impl ActivationHandler {
    pub(crate) fn set(&self, f: impl FnMut() + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    pub(crate) fn emit(&self) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f();
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// The content of one row. Plain text rows and custom widget rows are normalized
/// into the same type.
enum ListRowContent {
    Item(ListItem),
    Custom(Box<dyn Widget>),
}

impl Clone for ListRowContent {
    fn clone(&self) -> Self {
        match self {
            Self::Item(item) => Self::Item(item.clone()),
            Self::Custom(content) => Self::Custom(content.boxed_clone()),
        }
    }
}

/// How long the letters typed to jump to a row add up (milliseconds).
const TYPE_AHEAD_MILLIS: f64 = 1_000.0;

/// One row to put in a list.
///
/// [`ListItem`] is a convenience API for lists that need only strings. Rows that
/// combine icons, several labels, check boxes, trailing buttons and so on, as in a
/// settings screen, are built with `Grid` / `Stack` and wrapped in a `ListRow`.
///
/// ```no_run
/// # use naui_web::{ListRow, Widget};
/// # fn row(content: &dyn Widget) {
/// let row = ListRow::new(content).selectable(false);
/// row.on_activate(|| println!("row clicked"));
/// # let _ = row;
/// # }
/// ```
pub struct ListRow {
    content: ListRowContent,
    selectable: bool,
    activation: ActivationHandler,
}

impl Clone for ListRow {
    fn clone(&self) -> Self {
        Self {
            content: self.content.clone(),
            selectable: self.selectable,
            activation: self.activation.clone(),
        }
    }
}

impl ListRow {
    /// Uses a widget as the content of one row. By default the whole row can be
    /// selected too.
    pub fn new(content: &dyn Widget) -> Self {
        Self {
            content: ListRowContent::Custom(content.boxed_clone()),
            selectable: true,
            activation: ActivationHandler::default(),
        }
    }

    /// Converts a `ListItem` into a plain text row. The normalization path for
    /// `List::set_items`.
    fn from_item(item: ListItem) -> Self {
        let selectable = item.enabled;
        Self {
            content: ListRowContent::Item(item),
            selectable,
            activation: ActivationHandler::default(),
        }
    }

    /// Set to `false` for rows where only the controls inside are operated.
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    pub fn is_selectable(&self) -> bool {
        self.selectable
    }

    /// What to do when a label, icon or empty space inside the row is clicked.
    ///
    /// It is not called when a control such as a check box, button or text field is
    /// clicked directly, so the same action does not fire twice.
    pub fn on_activate(&self, f: impl FnMut() + 'static) {
        self.activation.set(f);
    }

    /// Whether a `<select>`'s `<option>` can show this row (text only, one line).
    fn fits_an_option(&self) -> bool {
        matches!(&self.content, ListRowContent::Item(item) if item.detail.is_none())
    }

    /// A row that can be neither selected nor operated (`ListItem::enabled(false)`).
    ///
    /// Custom-content rows are not dimmed, because the controls inside can still be
    /// clicked even when the whole row cannot be selected.
    fn is_dimmed(&self) -> bool {
        matches!(&self.content, ListRowContent::Item(item) if !item.enabled)
    }
}

/// Controls inside a row, distinguished from operations on the row itself.
///
/// The distinction the other platforms make through hit-testing and event routing
/// is made in the DOM by walking up the ancestors of the clicked element.
/// `<label>` operates the input inside it, so it is included here.
pub(crate) const ROW_CONTROLS: &str = "button, input, select, textarea, a[href], label, summary, \
[role=\"button\"], [contenteditable=\"true\"]";

/// Fields inside a row that keep the browser's own context menu (cut, paste and so
/// on). Right-clicking them does not become the row's context menu.
const TEXT_ENTRIES: &str = "input, textarea, [contenteditable=\"true\"]";

/// Whether the event was aimed at a control inside the row.
///
/// Used for key operations as well as clicks. Pass the row as `container` for
/// clicks, and the list itself for key operations.
pub(crate) fn control_was_targeted(container: &HtmlElement, event: &Event) -> bool {
    let Some(target) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
        return false;
    };
    let Ok(Some(control)) = target.closest(ROW_CONTROLS) else {
        return false;
    };
    // 内側で見つかったときだけ、コントロールの操作とみなす。
    !control.is_same_node(Some(container.as_ref())) && container.contains(Some(control.as_ref()))
}

/// The minimum `size`. 1 or less becomes a drop-down.
const MIN_ROWS: u32 = 2;

thread_local! {
    /// A per-list running number for referring to rows from
    /// `aria-activedescendant`.
    static NEXT_ID: Cell<u32> = const { Cell::new(0) };
}

fn next_list_id() -> u32 {
    NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    })
}

/// The spacing between the label and the secondary text.
const DETAIL_SPACING: f64 = 2.0;

/// The content of a text-only row.
///
/// The smallness and dimness of the secondary text come from
/// [`TextStyle::Caption`] and [`TextColor::Secondary`], so no CSS is written here.
pub(crate) fn item_content(doc: &Document, label: &str, detail: Option<&str>) -> Result<Stack> {
    let content = Stack::new(doc, Orientation::Vertical)?;
    content.set_align(Align::Fill);
    content.set_spacing(DETAIL_SPACING);
    content.append(&row_label(doc, label)?);
    if let Some(detail) = detail {
        let sub = row_label(doc, detail)?;
        sub.set_style(TextStyle::Caption);
        sub.set_color(TextColor::Secondary);
        content.append(&sub);
    }
    Ok(content)
}

/// One piece of text placed in a row.
///
/// **It stretches to the full width of the row.** By default `Label` does not wrap
/// and cuts the end with an ellipsis, but that works only once the width is
/// decided; at the content's width, a long label would stick out of the row
/// horizontally.
fn row_label(doc: &Document, text: &str) -> Result<Label> {
    let label = Label::new(doc, text)?;
    label.set_sizing(Sizing::fill_width());
    Ok(label)
}

fn style(element: &HtmlElement, property: &str, value: &str) {
    let _ = element.style().set_property(property, value);
}

/// The handler notified when the selection changes.
///
/// To use the same shape for single and multiple selection, the selected rows are
/// passed in ascending order. The closure is taken out only for the duration of the
/// call, so operating on the list from inside the callback does not double-borrow.
#[derive(Clone, Default)]
struct SelectionHandler(Rc<RefCell<Option<Box<dyn FnMut(&[usize])>>>>);

impl SelectionHandler {
    fn set(&self, f: impl FnMut(&[usize]) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, indices: &[usize]) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(indices);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// The body currently in use. [`List::set_items`] chooses again by looking at the
/// rows' contents.
enum Body {
    /// The browser's standard list box.
    Select {
        select: HtmlSelectElement,
        /// The `change` and `click` subscriptions. Dropping them also unsubscribes.
        _listeners: Vec<Listener>,
    },
    /// A composition for showing two-line rows and many rows.
    Listbox {
        list: HtmlElement,
        /// Fill in for the rows outside the window, keeping the scroll bar length.
        top_spacer: HtmlElement,
        bottom_spacer: HtmlElement,
        /// The `<li>`s of the built rows, in order from `window.start`.
        options: Vec<HtmlElement>,
        /// The click subscription of each built row, in the same order as `options`.
        row_listeners: Vec<Option<Listener>>,
        /// The subscriptions to the whole list (keys and scrolling).
        _listeners: Vec<Listener>,
    },
}

struct ListInner {
    /// The frame visible from outside. This element stays the same even when the
    /// contents are swapped.
    root: HtmlElement,
    document: Document,
    id: u32,
    body: RefCell<Body>,
    /// Where the rows come from.
    /// Marks the rows' cells and widgets as this list's (see [`crate::handover`]).
    owner: crate::handover::Owner,
    /// The rows built since the app last handed rows over ([`List::set_row_builder`],
    /// [`List::refresh`]) whose widgets were claimed. Building such a row again, when it
    /// scrolls back into view, does not take its widget back from a container the app
    /// has put it in since.
    claimed: RefCell<std::collections::HashSet<usize>>,
    rows: RowSource<ListRow>,
    /// The range of rows currently built. With many rows, only those around the screen.
    window: Cell<RowWindow>,
    /// Whether rows are being built right now. Prevents nested rebuilds.
    rebuilding: Cell<bool>,
    /// The contents of the built rows, in order from `window.start`. It also keeps the
    /// event subscriptions and the like contained in the rows alive.
    realized: RefCell<Vec<ListRow>>,
    /// The height of one row (logical pixels). Either measured or set with
    /// [`List::set_row_height`].
    row_height: Cell<f64>,
    /// Whether `row_height` is from rows that are gone, so the next measurement replaces
    /// it instead of only growing it.
    row_height_stale: Cell<bool>,
    /// The row [`List::scroll_to_row`] asked for while the list could not scroll to it
    /// (hidden, or not measured). It goes there once it can.
    pending_reveal: Cell<Option<usize>>,
    /// What has been typed to jump to a row, and when ([`List::type_ahead`]).
    typed: RefCell<String>,
    typed_at: Cell<f64>,
    /// Whether the last build stopped at a row it could not create, so the rows are
    /// incomplete even though the window has not moved.
    build_failed: Cell<bool>,
    /// The row height decided by the app. If absent, it is measured from the built rows.
    fixed_row_height: Cell<Option<f64>>,
    /// Moves the window when the list gets its size or it changes.
    resize_observer: RefCell<Option<ResizeObserver>>,
    resize_callback: RefCell<Option<Closure<dyn FnMut(JsValue)>>>,
    mode: Cell<SelectionMode>,
    /// The selected rows (ascending). This is the source of truth for both `<select>`
    /// and the composed list.
    selected: RefCell<Vec<usize>>,
    /// The row the keyboard is pointing at now. Used only for the composed list.
    active: Cell<Option<usize>>,
    /// The anchor of a Shift range selection.
    anchor: Cell<Option<usize>>,
    handler: SelectionHandler,
    /// The handler for double-clicks and Enter.
    row_activate: ValueHandler<usize>,
    /// The handler for right-clicks.
    context_menu: ValueHandler<(usize, Point)>,
    /// The `dblclick` / `contextmenu` / `keydown` subscriptions on the outer frame.
    _row_listeners: RefCell<Vec<Listener>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

impl Drop for ListInner {
    fn drop(&mut self) {
        // 捨てたあとに通知が届くと、解放した `Closure` を呼んでしまう。
        if let Some(observer) = self.resize_observer.borrow_mut().take() {
            observer.disconnect();
        }
    }
}

/// A vertical list of selectable rows.
///
/// Text-only rows become a `<select size>`; rows with `detail`, and lists with more than
/// [`ROW_WINDOW_THRESHOLD`] rows, become a `<ul role="listbox">`. An `Auto` height follows the number and content of the
/// rows; with a fixed height or `Fill`, it scrolls whatever overflows.
#[derive(Clone)]
pub struct List(Rc<ListInner>);
impl_widget!(List, root, after_sizing: |list: &List| list.update_window());

impl List {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let root: HtmlElement = create(doc, "div")?.unchecked_into();
        // 中身を枠いっぱいに広げる。枠は入れ替わらないので、
        // `set_sizing` の指定も親への追加もそのまま生き続ける。
        style(&root, "display", "flex");
        style(&root, "min-height", "0");

        let this = Self(Rc::new(ListInner {
            root,
            document: doc.clone(),
            id: next_list_id(),
            body: RefCell::new(Body::Select {
                select: create(doc, "select")?.unchecked_into(),
                _listeners: Vec::new(),
            }),
            rows: RowSource::default(),
            owner: crate::handover::Owner::new(),
            claimed: RefCell::new(std::collections::HashSet::new()),
            window: Cell::new(RowWindow::default()),
            rebuilding: Cell::new(false),
            realized: RefCell::new(Vec::new()),
            row_height: Cell::new(0.0),
            row_height_stale: Cell::new(false),
            pending_reveal: Cell::new(None),
            typed: RefCell::new(String::new()),
            typed_at: Cell::new(0.0),
            build_failed: Cell::new(false),
            fixed_row_height: Cell::new(None),
            resize_observer: RefCell::new(None),
            resize_callback: RefCell::new(None),
            mode: Cell::new(SelectionMode::Single),
            selected: RefCell::new(Vec::new()),
            active: Cell::new(None),
            anchor: Cell::new(None),
            handler: SelectionHandler::default(),
            row_activate: ValueHandler::default(),
            context_menu: ValueHandler::default(),
            _row_listeners: RefCell::new(Vec::new()),
            file_drop: Default::default(),
        }));
        this.build_select()?;
        this.listen_row_events()?;
        this.observe_size()?;
        Ok(this)
    }

    /// Rebuilds the rows. Indices change meaning, so the selection is cleared.
    ///
    /// If even one row has `detail`, or there are more than
    /// [`ROW_WINDOW_THRESHOLD`] rows, it is rebuilt as a `<ul role="listbox">`; otherwise
    /// as a `<select size>`.
    pub fn set_items(&self, items: &[ListItem]) {
        self.0
            .rows
            .set_rows(items.iter().cloned().map(ListRow::from_item).collect());
        self.reset_rows();
    }

    /// Rebuilds the rows. Both plain text rows and custom-content rows are built
    /// through this path.
    ///
    /// Controls inside a row can have their own callbacks as usual. Indices change
    /// meaning, so the selection is cleared.
    ///
    /// A row's widget is taken out of wherever it is now. It is put in its row each time
    /// the row is built, so if the app puts it in another container later without
    /// removing it from the list, the row is left empty from then on.
    pub fn set_rows(&self, rows: &[ListRow]) {
        self.0.claimed.borrow_mut().clear();
        for row in rows {
            self.claim(row);
        }
        self.0.rows.set_rows(rows.to_vec());
        self.reset_rows();
    }

    /// Passes rows in a form that **builds each row when it becomes visible**.
    ///
    /// `count` is the number of rows, and `build` receives an index from 0 to
    /// `count - 1` and returns that row ([`ListRow`]). It is called only for the rows on
    /// screen (and a few before and after), so opening stays fast even with hundreds of
    /// thousands of rows. Indices change meaning, so the selection is cleared.
    ///
    /// The widgets in a row may be recreated every time it is built. Keep per-row state
    /// in the app's data and build from it inside `build`.
    ///
    /// The first time each row is built after this call or [`List::refresh`], its
    /// widget is handed over the same way as with [`List::set_rows`]. Building it again
    /// when it scrolls back into view does not take it back from a container the app
    /// has put it in since.
    ///
    /// ```no_run
    /// # use naui_web::{List, ListRow, Ui};
    /// # fn fill(ui: &Ui, list: &List, cities: Vec<String>) {
    /// let ui = ui.clone();
    /// list.set_row_builder(cities.len(), move |index| {
    ///     Ok(ListRow::new(&ui.label(&cities[index])?))
    /// });
    /// # }
    /// ```
    pub fn set_row_builder(
        &self,
        count: usize,
        build: impl FnMut(usize) -> Result<ListRow> + 'static,
    ) {
        self.0.claimed.borrow_mut().clear();
        self.0.rows.set_builder(count, build);
        self.reset_rows();
    }

    /// Passes a function that answers whether a row can be selected **without building
    /// the row**.
    ///
    /// For rows built with [`List::set_row_builder`], naui does not know whether rows
    /// not yet built can be selected. With this function, [`List::set_selection`] can
    /// keep its promise of "removing rows that cannot be selected" even for rows off
    /// screen. Without it, rows follow the [`ListRow::selectable`] they were last built
    /// with, even after leaving the screen, and rows not built since the rows were passed
    /// or refreshed are treated as selectable.
    ///
    /// Its answers must match the [`ListRow::selectable`] returned by `build`. It stays when
    /// the rows are replaced, so pass a new one if its answers change.
    pub fn set_row_selectable(&self, selectable: impl Fn(usize) -> bool + 'static) {
        self.0.rows.set_selectable(selectable);
    }

    /// Rebuilds the visible rows. The row count and selection stay.
    ///
    /// Use it when what the function passed to [`List::set_row_builder`] returns has
    /// changed. The rows' widgets are handed over again.
    pub fn refresh(&self) {
        self.0.claimed.borrow_mut().clear();
        self.rebuild();
    }

    /// Takes a row's widget as this list's (see [`crate::handover`]).
    fn claim(&self, row: &ListRow) {
        if let ListRowContent::Custom(content) = &row.content {
            self.0.owner.claim(&content.native_element());
        }
    }

    /// Rebuilds the visible rows, without handing their widgets over again.
    fn rebuild(&self) {
        // `<select>` の行はアプリが渡したものなので、組み立て直すものが無い。
        if !matches!(&*self.0.body.borrow(), Body::Listbox { .. }) {
            return;
        }
        self.0.row_height_stale.set(true);
        self.0.rows.forget_selectable();
        // 行を組み立てる関数の中から呼ばれたら、組み立てている側が最初からやり直す。
        if self.0.rebuilding.get() {
            self.0.rows.invalidate();
            return;
        }
        let _ = self.build_window();
        // 中身や指定が変わって行の高さが動いていれば、窓の位置も変わる。測った結果が
        // 同じでも窓を引き直す (`set_row_height` は先に高さを書き換えている)。
        self.settle_row_height();
        self.update_window();
    }

    /// Sets the row height in logical pixels. Passing 0 or less measures it from the
    /// rows.
    ///
    /// Every row gets at least this height. With more rows than
    /// [`ROW_WINDOW_THRESHOLD`], rows off screen are filled in as "row count × this
    /// height", so set it when the rows' heights vary. A row built later that needs more
    /// raises the height for every row. A `<select>`'s `<option>`s cannot
    /// be given a height, so setting one makes the list composed.
    pub fn set_row_height(&self, height: f64) {
        let fixed = (height > 0.0).then_some(height);
        let changes_body = fixed.is_some() != self.0.fixed_row_height.get().is_some();
        self.0.fixed_row_height.set(fixed);
        // 指定を外したときは、組み立て直した行から測り直す。
        self.0.row_height.set(height.max(0.0));
        if changes_body {
            // 選択も残す。`set_rows` と違って行の意味は変わらない。
            let picked = self.selection();
            self.rebuild_body();
            self.write_selection(&picked);
            self.update_window();
        } else {
            self.rebuild();
        }
    }

    /// The scroll position of the body.
    fn scroll_top(&self) -> i32 {
        match &*self.0.body.borrow() {
            Body::Listbox { list, .. } => list.scroll_top(),
            Body::Select { select, .. } => select.scroll_top(),
        }
    }

    /// Moves the body's scroll position. The browser keeps it within the content.
    fn set_scroll_top(&self, top: i32) {
        match &*self.0.body.borrow() {
            Body::Listbox { list, .. } => list.set_scroll_top(top),
            Body::Select { select, .. } => select.set_scroll_top(top),
        }
    }

    /// The row height (logical pixels). 0 if there are no rows or none has been built yet.
    ///
    /// With few rows and no height set, rows may differ, and this is the tallest built.
    pub fn row_height(&self) -> f64 {
        if self.is_empty() && self.0.fixed_row_height.get().is_none() {
            return 0.0;
        }
        if matches!(&*self.0.body.borrow(), Body::Select { .. }) {
            self.settle_row_height();
        }
        self.0.row_height.get()
    }

    /// The number of rows.
    pub fn len(&self) -> usize {
        self.0.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Changes how rows are selected. The meaning of the selection changes, so the
    /// selection is cleared.
    pub fn set_selection_mode(&self, mode: SelectionMode) {
        self.0.mode.set(mode);
        self.apply_mode();
        self.write_selection(&[]);
    }

    pub fn selection_mode(&self) -> SelectionMode {
        self.0.mode.get()
    }

    /// The topmost of the selected rows.
    pub fn selected(&self) -> Option<usize> {
        self.0.selected.borrow().first().copied()
    }

    /// The selected rows (ascending). Zero or one for single selection.
    pub fn selection(&self) -> Vec<usize> {
        self.0.selected.borrow().clone()
    }

    /// Scrolls until row `index` is visible. The selection does not change.
    ///
    /// If it is already visible nothing moves; otherwise it is aligned to the nearer
    /// edge (the top if it is above, the bottom if it is below). Does nothing if out of
    /// range.
    pub fn scroll_to_row(&self, index: usize) {
        self.reveal_row(index);
    }

    /// Selects exactly one row without notifying.
    pub fn set_selected(&self, index: usize) {
        self.set_selection(&[index]);
    }

    /// Replaces the selection without notifying.
    ///
    /// Out-of-range rows, unselectable rows and duplicates are removed, and with single
    /// selection only the first remains ([`SelectionMode::normalize`]).
    pub fn set_selection(&self, indices: &[usize]) {
        let picked = self.normalize(indices);
        self.write_selection(&picked);
    }

    /// Clears the whole selection without notifying.
    pub fn clear_selection(&self) {
        self.write_selection(&[]);
    }

    /// Selects one row through the same path as a user's selection (notifies).
    pub fn select(&self, index: usize) {
        self.select_many(&[index]);
    }

    /// Replaces the selection through the same path as a user's selection (notifies).
    pub fn select_many(&self, indices: &[usize]) {
        self.set_selection(indices);
        // ブラウザはプログラムからの変更でイベントを出さないため、
        // ここで 1 回だけ通知する。
        let actual = self.selection();
        self.0.handler.emit(&actual);
    }

    /// Called with the selected rows (ascending) when the selection changes.
    ///
    /// With multiple selection it may be called with none.
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.handler.set(f);
    }

    /// Called with the row's index when a row is activated (double-clicked, or Enter
    /// while the list has focus).
    ///
    /// With Enter, the target is the row the keyboard points at (for `<select>`, the
    /// first selected row). To open everything in a multiple selection, read
    /// [`List::selection`] inside it. Not called for rows of `ListItem`s that cannot be
    /// selected (`enabled(false)`), nor over controls inside a row.
    pub fn on_row_activate(&self, f: impl FnMut(usize) + 'static) {
        self.0.row_activate.set(f);
    }

    /// Called with the row's index and the click position when a row is right-clicked.
    ///
    /// The position is in logical pixels from the top left of this list and can be
    /// passed straight to [`crate::PopupMenu::open_at`]. Once set, the browser's default
    /// menu is suppressed over rows (not over fields inside a row, which are neither
    /// suppressed nor notified). The selection does not change. Not called for empty
    /// space without rows.
    pub fn on_context_menu(&self, mut f: impl FnMut(usize, Point) + 'static) {
        self.0
            .context_menu
            .set(move |(index, point)| f(index, point));
    }

    /// The inner `<select>`. Public as a backend-specific escape hatch.
    ///
    /// **Only a list of few text-only rows has one.** The list is a `<ul role="listbox">`
    /// instead, and this returns `None`, in any of these cases, so it can change whenever
    /// the rows change:
    ///
    /// - there are more than [`ROW_WINDOW_THRESHOLD`] rows
    /// - some row has `detail` or is a [`ListRow`] of arbitrary content
    /// - the rows come from [`List::set_row_builder`]
    /// - a height is set with [`List::set_row_height`]
    ///
    /// The frame itself can be obtained from [`Widget::native_element`].
    pub fn native_select(&self) -> Option<HtmlSelectElement> {
        match &*self.0.body.borrow() {
            Body::Select { select, .. } => Some(select.clone()),
            Body::Listbox { .. } => None,
        }
    }

    // ------------------------------------------------------------ 組み立て

    /// Creates the browser's standard `<select size>`. The structure for text-only
    /// rows.
    fn build_select(&self) -> Result<()> {
        let select: HtmlSelectElement = create(&self.0.document, "select")?.unchecked_into();
        let count = self.len();
        for index in 0..count {
            let Some(Some(item)) = self.0.rows.with_stored(index, |row| match &row.content {
                ListRowContent::Item(item) => Some(item.clone()),
                ListRowContent::Custom(_) => None,
            }) else {
                continue;
            };
            let option: HtmlOptionElement = create(&self.0.document, "option")?.unchecked_into();
            option.set_text_content(Some(&item.label));
            option.set_disabled(!item.enabled);
            let _ = select.append_child(&option);
        }
        // `size` が無いとドロップダウンになるので、必ず 2 以上を入れる。
        let size = u32::try_from(count).unwrap_or(u32::MAX).max(MIN_ROWS);
        select.set_size(size);

        let mut listeners = Vec::with_capacity(2);
        // 選択はブラウザが動かすので、その結果を読み取って通知するだけ。
        if let Ok(listener) = Listener::attach(select.as_ref(), "change", {
            let weak = Rc::downgrade(&self.0);
            move || {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let list = List(inner);
                let picked = list.read_select_selection();
                *list.0.selected.borrow_mut() = picked.clone();
                list.0.handler.emit(&picked);
            }
        }) {
            listeners.push(listener);
        }
        // `<option>` の中はテキストだけで、行内のコントロールも無い。
        // 押された `<option>` を辿って、その行の activation を出す。
        if let Ok(listener) = Listener::attach_event(select.as_ref(), "click", {
            let weak = Rc::downgrade(&self.0);
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let Some(index) = clicked_option_index(&event) else {
                    return;
                };
                let activation = List(inner).read_row(index, |row| row.activation.clone());
                if let Some(activation) = activation {
                    activation.emit();
                }
            }
        }) {
            listeners.push(listener);
        }

        self.swap_body(select.as_ref())?;
        *self.0.body.borrow_mut() = Body::Select {
            select,
            _listeners: listeners,
        };
        self.apply_mode();
        Ok(())
    }

    /// Builds the `role="listbox"`, without its rows. Two-line rows, custom-content rows
    /// and many rows go through here.
    fn build_listbox(&self) -> Result<()> {
        let list: HtmlElement = create(&self.0.document, "ul")?.unchecked_into();
        let _ = list.set_attribute("role", "listbox");
        // キーボードで入れるようにする。中の行は `aria-activedescendant` で指す。
        let _ = list.set_attribute("tabindex", "0");
        style(&list, "list-style", "none");
        style(&list, "margin", "0");
        style(&list, "padding", "0");
        style(&list, "overflow-y", "auto");
        // 行の `offsetTop` がこの要素を基準になるようにする
        // (`reveal_active` がスクロール位置を求めるのに使う)。
        style(&list, "position", "relative");
        // 窓を動かすと、残る行の上で詰め物と行が入れ替わる。スクロール位置は詰め物の
        // 高さから求めているので、ブラウザに補正させない。
        style(&list, "overflow-anchor", "none");
        // 枠と地の色は、ブラウザが入力欄に使うシステム色に任せる。
        // 線は `GrayText` にする。Safari (WebKit) の `ButtonBorder` は明暗
        // どちらの配色でも白に解決されるため、明るい配色では枠が消え、
        // 暗い配色では白く出すぎる (`SplitView` の仕切りや `Table` と同じ判断)。
        style(&list, "border", "1px solid");
        style(&list, "border-color", "GrayText");
        style(&list, "background-color", "Field");
        style(&list, "color", "FieldText");

        let top_spacer = spacer_item(&self.0.document)?;
        let bottom_spacer = spacer_item(&self.0.document)?;
        let _ = list.append_child(&top_spacer);
        let _ = list.append_child(&bottom_spacer);

        let mut listeners = Vec::with_capacity(2);
        listeners.push(Listener::attach_event(list.as_ref(), "keydown", {
            let weak = Rc::downgrade(&self.0);
            let list = list.clone();
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                // 行内のコントロールが受けたキーは、そのコントロールのもの。
                // リスト操作として横取りすると、チェックボックスの Space や
                // 入力欄の矢印キーが動かなくなる。
                if control_was_targeted(&list, &event) {
                    return;
                }
                if let Some(key) = event.dyn_ref::<KeyboardEvent>() {
                    if List(inner).on_key(key) {
                        event.prevent_default();
                    }
                }
            }
        })?);
        // 行が多いときは、スクロールに合わせて組み立てる範囲を動かす。
        listeners.push(Listener::attach(list.as_ref(), "scroll", {
            let weak = Rc::downgrade(&self.0);
            move || {
                if let Some(inner) = weak.upgrade() {
                    List(inner).update_window();
                }
            }
        })?);

        self.swap_body(&list)?;
        *self.0.body.borrow_mut() = Body::Listbox {
            list,
            top_spacer,
            bottom_spacer,
            options: Vec::new(),
            row_listeners: Vec::new(),
            _listeners: listeners,
        };
        self.apply_mode();
        Ok(())
    }

    /// Builds one `<li>` and its click subscription.
    ///
    /// Rows outside the window have no `<li>`, so `index` is the row's place in the whole
    /// list, not among the rows built.
    fn build_option(
        &self,
        list: &HtmlElement,
        index: usize,
        row: &ListRow,
    ) -> Result<(HtmlElement, Option<Listener>)> {
        let window = self.0.window.get();
        let option: HtmlElement = create(&self.0.document, "li")?.unchecked_into();
        let _ = option.set_attribute("role", "option");
        let _ = option.set_attribute("id", &self.option_id(index));
        let _ = option.set_attribute("aria-selected", "false");
        // 読み上げには、窓ではなくリストそのものの位置を伝える。
        let _ = option.set_attribute("aria-setsize", &window.count.to_string());
        let _ = option.set_attribute("aria-posinset", &(index + 1).to_string());
        // 行の高さをそろえるときの指定に、余白も含める。
        style(&option, "box-sizing", "border-box");
        if let Some(height) = self.0.fixed_row_height.get() {
            style(&option, "min-height", &format!("{height}px"));
        }
        match &row.content {
            ListRowContent::Item(item) => {
                style(&option, "padding", "2px 4px");
                let content = item_content(&self.0.document, &item.label, item.detail.as_deref())?;
                let _ = option.append_child(&content.native_element());
            }
            ListRowContent::Custom(content) => {
                style(&option, "padding", "6px 10px");
                // 外さずにほかへ移った中身は、移った先に残して空の行にする。
                self.0.owner.mark_cell(&option);
                let content = content.native_element();
                if self.0.owner.may_take(&content) {
                    let _ = option.append_child(&content);
                }
            }
        }

        if !row.selectable {
            let _ = option.set_attribute("aria-disabled", "true");
        }
        if row.is_dimmed() {
            // 無効な文字にブラウザが使うシステム色。
            style(&option, "color", "GrayText");
            // 行そのものも中身も操作できないので、購読も付けない。
            return Ok((option, None));
        }

        let selectable = row.selectable;
        let listener = Listener::attach_event(option.as_ref(), "click", {
            let weak = Rc::downgrade(&self.0);
            let option = option.clone();
            let list = list.clone();
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let this = List(inner);
                // Button や input 自身の操作はそのまま通し、行そのものが
                // 押されたときだけ activation を出す。
                if !control_was_targeted(&option, &event) {
                    // クリックの続きを矢印キーで行えるように、
                    // リストへフォーカスを移す (行内のコントロールを
                    // 押したときは、そのコントロールから奪わない)。
                    let _ = list.focus();
                    let activation = this.read_row(index, |row| row.activation.clone());
                    if let Some(activation) = activation {
                        activation.emit();
                    }
                }
                if !selectable {
                    return;
                }
                // 行選択は通常のクリックと同じ規則で更新する。
                let mouse = event.dyn_ref::<MouseEvent>();
                let toggle = mouse.is_some_and(|e| e.meta_key() || e.ctrl_key());
                let extend = mouse.is_some_and(|e| e.shift_key());
                this.on_row_activated(index, toggle, extend);
            }
        })?;
        Ok((option, Some(listener)))
    }

    /// Moves the window when the list gets its size (it has none until it is placed in
    /// the document) or its size changes.
    fn observe_size(&self) -> Result<()> {
        let weak = Rc::downgrade(&self.0);
        let callback = Closure::<dyn FnMut(JsValue)>::new(move |_entries: JsValue| {
            if let Some(inner) = weak.upgrade() {
                List(inner).update_window();
            }
        });
        let observer = ResizeObserver::new(callback.as_ref().unchecked_ref())
            .map_err(|e| crate::to_error("create the ResizeObserver", e))?;
        observer.observe(self.0.root.as_ref());
        *self.0.resize_observer.borrow_mut() = Some(observer);
        *self.0.resize_callback.borrow_mut() = Some(callback);
        Ok(())
    }

    // ------------------------------------------------------- 行を絞る窓

    /// Rebuilds the rows where the list is scrolled to. The selection is cleared.
    fn reset_rows(&self) {
        self.0.selected.borrow_mut().clear();
        self.0.active.set(None);
        self.0.anchor.set(None);
        self.0.pending_reveal.set(None);
        self.0.typed.borrow_mut().clear();
        self.rebuild_body();
    }

    /// Chooses the body for the current rows and builds it, keeping the scroll position
    /// as the other platforms do.
    fn rebuild_body(&self) {
        // 組み立ての途中で呼ばれたら、組み立てている側が中身ごと作り直す。
        if self.0.rebuilding.get() {
            self.0.realized.borrow_mut().clear();
            self.0.rows.invalidate();
            return;
        }
        let top = self.scroll_top();
        if self.prepare_body(top).is_err() {
            return;
        }
        if matches!(&*self.0.body.borrow(), Body::Listbox { .. }) {
            let _ = self.build_window();
            self.settle_row_height();
        }
        // 作り直した要素は先頭にある。詰め物の高さが決まってから戻し、そこの行を
        // 組み立てる。
        self.set_scroll_top(top);
        self.update_window();
    }

    /// Replaces the body with the one the current rows need, and chooses the window.
    /// A `<select>` is complete after this; a list box still needs its rows built, for the
    /// window at `top`, where the caller scrolls it once it is tall enough.
    fn prepare_body(&self, top: i32) -> Result<()> {
        self.0.row_height_stale.set(true);
        self.0.realized.borrow_mut().clear();
        // `<option>` はテキストしか持てず、作った分だけ重くなり、高さも指定できない。
        // そのどれにも当たらないときだけブラウザ標準の `<select>` を使い続ける。
        let plain = !self.0.rows.is_built()
            && self.len() <= ROW_WINDOW_THRESHOLD
            && self.0.fixed_row_height.get().is_none()
            && !self.0.rows.any_stored(|row| !row.fits_an_option());
        if plain {
            self.0.window.set(RowWindow::all(self.len()));
            return self.build_select();
        }
        self.build_listbox()?;
        self.0
            .window
            .set(self.compute_window_at(top, ROW_WINDOW_OVERSCAN));
        Ok(())
    }

    /// Rebuilds every `<li>` of the current window.
    ///
    /// Rows outside the window get no `<li>`; the spacers above and below carry their
    /// height, so the scroll bar's length and position are those of all the rows.
    fn build_window(&self) -> Result<()> {
        self.build_window_from(None)
    }

    /// Builds the current window. The rows `previous` has in common with it stay as they
    /// are, unless the rows are replaced during the build.
    fn build_window_from(&self, previous: Option<RowWindow>) -> Result<()> {
        // 組み立ての途中でアプリのコードが動くので、そこから呼ばれても
        // 二重に作り直さない。
        if self.0.rebuilding.replace(true) {
            return Ok(());
        }
        let top = self.scroll_top();
        let mut started_over = false;
        let mut result = Ok(());
        self.0.rows.build_until_settled(|replaced| {
            started_over |= replaced;
            let prepared = match replaced {
                true => self.prepare_body(top),
                false => Ok(()),
            };
            result = prepared.and_then(|()| self.build_window_once(previous.filter(|_| !replaced)));
        });
        self.0.rebuilding.set(false);
        // 組み立て直すと要素ごと新しくなり、先頭に戻っている。
        if started_over {
            self.set_scroll_top(top);
        }
        result
    }

    fn build_window_once(&self, previous: Option<RowWindow>) -> Result<()> {
        let window = self.0.window.get();
        let generation = self.0.rows.generation();
        // アプリのコードが動く間は、中身の借用を持たない。
        let Some((list, bottom_spacer, old, old_listeners)) = (match &mut *self.0.body.borrow_mut()
        {
            Body::Listbox {
                list,
                bottom_spacer,
                options,
                row_listeners,
                ..
            } => Some((
                list.clone(),
                bottom_spacer.clone(),
                std::mem::take(options),
                std::mem::take(row_listeners),
            )),
            Body::Select { .. } => None,
        }) else {
            return Ok(());
        };
        // 窓に残る行は触らない。作り直すと、中のコントロールのフォーカスが失われる。
        // 前回の組み立てが途中で終わっていたら、位置が合わないので全部作り直す。
        // 全行を組み立てるときも、そろえた高さから行ごとの高さへ戻すために作り直す。
        let previous =
            previous.filter(|previous| previous.len() == old.len() && !window.is_complete());
        let kept = previous.map_or(0..0, |previous| previous.overlap(window));
        let built_start = previous.map_or(0, |previous| previous.start);

        let mut options = Vec::with_capacity(window.len());
        let mut listeners = Vec::with_capacity(window.len());
        let mut realized = Vec::with_capacity(window.len());
        let focused = self.0.document.active_element();
        let mut lost_focus = false;
        for (offset, (option, listener)) in old.into_iter().zip(old_listeners).enumerate() {
            let index = built_start + offset;
            if !kept.contains(&index) {
                lost_focus |= option.contains(focused.as_ref().map(|focused| focused.as_ref()));
                option.remove();
                continue;
            }
            options.push(option);
            listeners.push(listener);
            realized.extend(self.0.realized.borrow().get(offset).cloned());
        }
        // 詰め物は残したまま行だけを入れ替える。
        let first_kept = options.first().cloned();
        let mut before_kept = 0;
        let mut outcome = Ok(());
        for index in window.indices().filter(|index| !kept.contains(index)) {
            let Some(row) = self.0.rows.row(index) else {
                continue;
            };
            // 差し替えられたら、この行も残りの窓ももう古い。
            if self.0.rows.generation() != generation {
                break;
            }
            // 組み立てに失敗した行は、空の行にする。
            let row = row.unwrap_or_else(|_| ListRow::from_item(ListItem::new("")));
            // アプリが行を渡し直してから初めて組み立てた行は、中身を引き取る。
            // 見えるところへ戻ってきたための組み立てでは引き取らない。
            if self.0.rows.is_built() && self.0.claimed.borrow_mut().insert(index) {
                self.claim(&row);
            }
            self.0.rows.note_selectable(index, row.is_selectable());
            // 失敗しても、ここまでの行は文書に載っているので、管理から外さない。
            // 行数が窓と合わないので、次の組み立てで全部作り直す。
            let (option, listener) = match self.build_option(&list, index, &row) {
                Ok(built) => built,
                Err(error) => {
                    outcome = Err(error);
                    break;
                }
            };
            if index < kept.start {
                let _ = list.insert_before(&option, first_kept.as_ref().map(|row| row.as_ref()));
                options.insert(before_kept, option);
                listeners.insert(before_kept, listener);
                realized.insert(before_kept, row);
                before_kept += 1;
            } else {
                let _ = list.insert_before(&option, Some(&bottom_spacer));
                options.push(option);
                listeners.push(listener);
                realized.push(row);
            }
        }
        *self.0.realized.borrow_mut() = realized;
        if let Body::Listbox {
            options: stored,
            row_listeners,
            ..
        } = &mut *self.0.body.borrow_mut()
        {
            *stored = options;
            *row_listeners = listeners;
        }
        self.write_spacers();
        self.sync_active_descendant();
        // 覚えている選択を、組み立て直した行へ写す。
        let picked = self.selection();
        self.write_selection(&picked);
        self.0.build_failed.set(outcome.is_err());
        // 行といっしょにフォーカスが消えると、キーがどこにも届かなくなる。
        if lost_focus {
            let _ = list.focus();
        }
        outcome
    }

    /// Writes the height of the rows outside the window into the spacers.
    fn write_spacers(&self) {
        let window = self.0.window.get();
        let height = self.0.row_height.get();
        if let Body::Listbox {
            top_spacer,
            bottom_spacer,
            ..
        } = &*self.0.body.borrow()
        {
            set_spacer_height(top_spacer, window.leading(height));
            set_spacer_height(bottom_spacer, window.trailing(height));
        }
    }

    /// Computes the range of rows to build from the current scroll position.
    fn compute_window(&self, overscan: usize) -> RowWindow {
        self.compute_window_at(self.scroll_top(), overscan)
    }

    /// Computes the range of rows to build at the scroll position `top`.
    fn compute_window_at(&self, top: i32, overscan: usize) -> RowWindow {
        let viewport = match &*self.0.body.borrow() {
            Body::Listbox { list, .. } => list.client_height(),
            Body::Select { .. } => return RowWindow::all(self.len()),
        };
        let limited = crate::layout::height_is_limited(&self.0.root);
        // `Auto` の高さで知りたいのは、大きさが決まる場所にあるかどうか。作り直したばかりの
        // `<ul>` は空で内側の高さが 0 なので、枠に聞く (`<ul>` の線の分があるので、文書に
        // 載って表示されていれば 0 にならない)。
        let viewport = match limited {
            true => viewport,
            false => self.0.root.client_height(),
        };
        list_row_window(
            limited,
            self.len(),
            f64::from(top),
            f64::from(viewport),
            self.0.row_height.get(),
            overscan,
        )
    }

    /// Moves the built range along with scrolling and resizing.
    ///
    /// Rebuilding for every row scrolled is heavy, so the window is reused until you
    /// have scrolled through half of the extra rows ([`ROW_WINDOW_OVERSCAN`]). Even then,
    /// only the rows that were not there are built.
    fn update_window(&self) {
        if self.0.rebuilding.get() {
            return;
        }
        let shown = match &*self.0.body.borrow() {
            Body::Listbox { list, .. } => list.client_height() > 0,
            Body::Select { .. } => false,
        };
        // 隠れている間は位置も大きさも読めないので、組み立てた行をそのまま残す。
        if !shown {
            return;
        }
        // 文書に載る前に組み立てた行は、ここで初めて測れる。
        if self.0.row_height.get() <= 0.0 {
            self.settle_row_height();
        }
        if let Some(index) = self.0.pending_reveal.take() {
            self.reveal_row(index);
        }
        let current = self.0.window.get();
        let needed = self.compute_window(ROW_WINDOW_OVERSCAN / 2);
        let next = self.compute_window(ROW_WINDOW_OVERSCAN);
        // 前回の組み立てが途中で失敗していたら、窓が動かなくても組み立て直す。
        if !self.0.build_failed.get()
            && (next == current || keeps_row_window(current, needed, next))
        {
            return;
        }
        self.0.window.set(next);
        let _ = self.build_window_from(Some(current));
        if self.settle_row_height() {
            self.update_window();
        }
    }

    /// Finds the height of one row from the built rows. `true` if it changed.
    ///
    /// The browser decides row heights, so they are not known until a row is built.
    /// **The tallest built row is used, and every built row is given that height.**
    /// The rows outside the window are filled in as "row count × this height", so rows
    /// of differing height (only some with `detail`, for instance) would make the
    /// scroll bar drift every time the window moves. A taller row appearing later grows
    /// the height once.
    fn settle_row_height(&self) -> bool {
        let window = self.0.window.get();
        let body = self.0.body.borrow();
        let tallest = match &*body {
            Body::Listbox { options, .. } => options
                .iter()
                .map(|option| option.get_bounding_client_rect().height())
                .fold(0.0, f64::max),
            Body::Select { select, .. } => select
                .options()
                .item(0)
                .map_or(0.0, |option| option.get_bounding_client_rect().height()),
        };
        let previous = self.0.row_height.get();
        // 文書に載る前は 0 が返る。古い高さを捨てず、測れるようになるのを待つ。
        if tallest <= 0.0 {
            return false;
        }
        // 指定は下限。中身がそれより高い行は伸びるので、窓の計算もそちらに合わせる。
        let measured = match self.0.row_height_stale.replace(false) {
            true => tallest,
            false => tallest.max(previous),
        }
        .max(self.0.fixed_row_height.get().unwrap_or(0.0));
        // 行が少なければ行ごとの高さのまま (窓の外の行が無いので、ずれない)。
        if let (Body::Listbox { options, .. }, false) = (&*body, window.is_complete()) {
            for option in options {
                style(option, "min-height", &format!("{measured}px"));
            }
        }
        drop(body);
        self.0.row_height.set(measured);
        self.write_spacers();
        (previous - measured).abs() >= 0.5
    }

    /// Reads a row without building anything: one the app passed, or one built now.
    fn read_row<R>(&self, index: usize, read: impl FnOnce(&ListRow) -> R) -> Option<R> {
        if !self.0.rows.is_built() {
            return self.0.rows.with_stored(index, read);
        }
        let offset = index.checked_sub(self.0.window.get().start)?;
        self.0.realized.borrow().get(offset).map(read)
    }

    /// Receives row activation and right-clicks on the outer frame. Not reattached when
    /// the contents are rebuilt.
    fn listen_row_events(&self) -> Result<()> {
        let root: &web_sys::EventTarget = self.0.root.as_ref();
        let mut listeners = Vec::with_capacity(3);
        listeners.push(Listener::attach_event(root, "dblclick", {
            let weak = Rc::downgrade(&self.0);
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let list = List(inner);
                if let Some((index, row)) = list.row_of_event(&event) {
                    if !control_was_targeted(&row, &event) {
                        list.0.row_activate.emit(index);
                    }
                }
            }
        })?);
        listeners.push(Listener::attach_event(root, "contextmenu", {
            let weak = Rc::downgrade(&self.0);
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let list = List(inner);
                if !list.0.context_menu.is_set() {
                    return;
                }
                let Some((index, row)) = list.row_of_event(&event) else {
                    return;
                };
                if entry_was_targeted(&row, &event) {
                    return;
                }
                let Some(mouse) = event.dyn_ref::<MouseEvent>() else {
                    return;
                };
                let rect = list.0.root.get_bounding_client_rect();
                let point = Point::new(
                    f64::from(mouse.client_x()) - rect.left(),
                    f64::from(mouse.client_y()) - rect.top(),
                );
                // ブラウザ既定のメニューも、`PopupMenu::attach` で同じ枠や
                // 祖先に付けたメニューも出さない (この購読は枠を作るときに
                // 付けるので、同じ枠のほかの購読より先に呼ばれる)。
                event.prevent_default();
                event.stop_immediate_propagation();
                list.0.context_menu.emit((index, point));
            }
        })?);
        listeners.push(Listener::attach_event(root, "keydown", {
            let weak = Rc::downgrade(&self.0);
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let Some(key) = event.dyn_ref::<KeyboardEvent>() else {
                    return;
                };
                let modified = key.shift_key() || key.ctrl_key() || key.alt_key() || key.meta_key();
                if key.key() != "Enter" || key.is_composing() || modified {
                    return;
                }
                let list = List(inner);
                if let Some(index) = list.keyboard_row(&event) {
                    list.0.row_activate.emit(index);
                }
            }
        })?);
        *self.0._row_listeners.borrow_mut() = listeners;
        Ok(())
    }

    /// The row the event was aimed at, and that row's element. `None` for rows that
    /// receive neither activation nor right-clicks.
    ///
    /// The row's position is looked up from the clicked element on the spot (so it stays
    /// correct even if the rows are rebuilt).
    fn row_of_event(&self, event: &Event) -> Option<(usize, HtmlElement)> {
        let target = event.target()?.dyn_into::<Element>().ok()?;
        let (index, row) = match &*self.0.body.borrow() {
            Body::Select { .. } => {
                let option = target.closest("option").ok()??;
                let index = option.clone().dyn_into::<HtmlOptionElement>().ok()?.index();
                (usize::try_from(index).ok()?, option.dyn_into().ok()?)
            }
            Body::Listbox { options, .. } => {
                let option = target.closest("[role=\"option\"]").ok()??;
                let offset = options
                    .iter()
                    .position(|row| row.is_same_node(Some(option.as_ref())))?;
                (self.0.window.get().start + offset, options[offset].clone())
            }
        };
        self.is_live(index).then_some((index, row))
    }

    /// The row activated by Enter: the row the keyboard points at, or the first
    /// selected row.
    fn keyboard_row(&self, event: &Event) -> Option<usize> {
        let index = match &*self.0.body.borrow() {
            Body::Select { select, .. } => usize::try_from(select.selected_index()).ok()?,
            Body::Listbox { list, .. } => {
                // 行内のコントロールが受けた Enter は、そのコントロールのもの。
                if control_was_targeted(list, event) {
                    return None;
                }
                self.0.active.get().or_else(|| self.selected())?
            }
        };
        self.is_live(index).then_some(index)
    }

    /// Whether the row can receive activation and right-clicks (not an unselectable
    /// `ListItem`).
    fn is_live(&self, index: usize) -> bool {
        index < self.len() && !self.read_row(index, ListRow::is_dimmed).unwrap_or(false)
    }

    /// Swaps the contents of the frame.
    fn swap_body(&self, element: &HtmlElement) -> Result<()> {
        while let Some(child) = self.0.root.last_element_child() {
            let _ = self.0.root.remove_child(&child);
        }
        style(element, "flex", "1");
        style(element, "min-width", "0");
        self.0
            .root
            .append_child(element)
            .map(|_| ())
            .map_err(|e| crate::to_error("build the list", e))
    }

    fn option_id(&self, index: usize) -> String {
        format!("naui-list-{}-row-{index}", self.0.id)
    }

    /// Applies the single / multiple setting to the current body.
    fn apply_mode(&self) {
        let multiple = self.0.mode.get().is_multiple();
        match &*self.0.body.borrow() {
            Body::Select { select, .. } => select.set_multiple(multiple),
            Body::Listbox { list, .. } => {
                let _ = list.set_attribute(
                    "aria-multiselectable",
                    if multiple { "true" } else { "false" },
                );
            }
        }
    }

    // --------------------------------------------------------------- 選択

    /// Reads the `<select>`'s selection from the DOM.
    fn read_select_selection(&self) -> Vec<usize> {
        let body = self.0.body.borrow();
        let Body::Select { select, .. } = &*body else {
            return Vec::new();
        };
        let options = select.options();
        (0..options.length())
            .filter(|&i| {
                options
                    .get_with_index(i)
                    .and_then(|node| node.dyn_into::<HtmlOptionElement>().ok())
                    .is_some_and(|option| option.selected())
            })
            .map(|i| i as usize)
            .collect()
    }

    fn normalize(&self, indices: &[usize]) -> Vec<usize> {
        self.0
            .mode
            .get()
            .normalize_by(indices, |index| self.is_selectable(index))
    }

    /// Whether the row can be selected.
    fn is_selectable(&self, index: usize) -> bool {
        self.0.rows.is_selectable(index, ListRow::is_selectable)
    }

    /// Remembers the selection and writes it into the body as is (no notification).
    fn write_selection(&self, indices: &[usize]) {
        *self.0.selected.borrow_mut() = indices.to_vec();
        let body = self.0.body.borrow();
        match &*body {
            Body::Select { select, .. } => {
                let options = select.options();
                for i in 0..options.length() {
                    if let Some(option) = options
                        .get_with_index(i)
                        .and_then(|node| node.dyn_into::<HtmlOptionElement>().ok())
                    {
                        option.set_selected(indices.contains(&(i as usize)));
                    }
                }
            }
            Body::Listbox { options, .. } => {
                let start = self.0.window.get().start;
                for (offset, option) in options.iter().enumerate() {
                    let index = start + offset;
                    let picked = indices.binary_search(&index).is_ok();
                    let _ = option
                        .set_attribute("aria-selected", if picked { "true" } else { "false" });
                    if picked {
                        // 選択の色はブラウザのシステム色をそのまま使う。
                        // 新しい名前に対応していれば、後の指定が勝つ。
                        style(option, "background-color", "Highlight");
                        style(option, "background-color", "SelectedItem");
                        style(option, "color", "HighlightText");
                        style(option, "color", "SelectedItemText");
                    } else {
                        style(option, "background-color", "");
                        style(option, "color", "");
                        if self.read_row(index, ListRow::is_dimmed).unwrap_or(false) {
                            style(option, "color", "GrayText");
                        }
                    }
                }
            }
        }
    }

    /// When a row of the composed list is clicked.
    fn on_row_activated(&self, index: usize, toggle: bool, extend: bool) {
        let multiple = self.0.mode.get().is_multiple();
        let picked = if multiple && extend {
            let anchor = self.0.anchor.get().unwrap_or(index);
            self.range(anchor, index)
        } else if multiple && toggle {
            let mut picked = self.selection();
            match picked.iter().position(|&i| i == index) {
                Some(at) => {
                    picked.remove(at);
                }
                None => picked.push(index),
            }
            self.0.anchor.set(Some(index));
            picked
        } else {
            self.0.anchor.set(Some(index));
            vec![index]
        };
        self.0.active.set(Some(index));
        self.commit(&picked);
    }

    /// Key operations. Returns `true` if handled (preventing the browser's default
    /// action).
    fn on_key(&self, event: &KeyboardEvent) -> bool {
        let len = self.len();
        if len == 0 {
            return false;
        }
        let current = self.0.active.get().or_else(|| self.selected());
        if let Some(target) = self.current_row_moved_by(event, current) {
            self.0.active.set(Some(target));
            self.reveal_active();
            return true;
        }
        let target = match event.key().as_str() {
            "ArrowDown" => self.step(current, 1),
            "ArrowUp" => self.step(current, -1),
            "Home" => self.first_enabled(0, 1),
            "End" => self.first_enabled(len as isize - 1, -1),
            // 修飾キーが付いたらブラウザの操作 (タブの切り替えなど) なので、奪わない。
            "PageDown" | "PageUp" if event.ctrl_key() || event.meta_key() || event.alt_key() => {
                return false;
            }
            "PageDown" => self.page(current, 1),
            "PageUp" => self.page(current, -1),
            "a" | "A"
                if (event.ctrl_key() || event.meta_key())
                    && !event.shift_key()
                    && !event.alt_key()
                    && self.0.mode.get().is_multiple() =>
            {
                let every: Vec<usize> = (0..len).collect();
                let picked = self.normalize(&every);
                if picked != self.selection() {
                    self.write_selection(&picked);
                    self.0.handler.emit(&picked);
                }
                return true;
            }
            // 複数選択では Space で、いま指している行を入れたり外したりする。
            " " if self.0.mode.get().is_multiple() => {
                let Some(index) = current else {
                    return false;
                };
                // Ctrl で選べない行にも止まれる。
                if self.is_selectable(index) {
                    self.on_row_activated(index, true, false);
                }
                return true;
            }
            _ => {
                let Some(target) = self.type_ahead(event, current) else {
                    return false;
                };
                // Shift は大文字を打つためのもので、範囲を選ぶためではない。
                self.0.active.set(Some(target));
                self.0.anchor.set(Some(target));
                self.commit(&[target]);
                return true;
            }
        };
        let Some(target) = target else {
            return false;
        };
        self.0.active.set(Some(target));
        if self.0.mode.get().is_multiple() && event.shift_key() {
            let anchor = self.0.anchor.get().unwrap_or(target);
            let picked = self.range(anchor, target);
            self.commit(&picked);
        } else {
            self.0.anchor.set(Some(target));
            self.commit(&[target]);
        }
        true
    }

    /// Where Ctrl (⌘) with an arrow, Home or End moves the row the keyboard points at. The
    /// selection stays, so rows can be picked one by one with Space, and the row can be one
    /// that cannot be selected. `None` for any other key, and with single selection, which
    /// has no way to add a row.
    fn current_row_moved_by(&self, event: &KeyboardEvent, current: Option<usize>) -> Option<usize> {
        if !(event.ctrl_key() || event.meta_key())
            || event.shift_key()
            || !self.0.mode.get().is_multiple()
        {
            return None;
        }
        let last = self.len() - 1;
        match event.key().as_str() {
            "ArrowDown" => Some(current.map_or(0, |index| (index + 1).min(last))),
            "ArrowUp" => Some(current.map_or(last, |index| index.saturating_sub(1))),
            "Home" => Some(0),
            "End" => Some(last),
            _ => None,
        }
    }

    /// The row a page away from `from` for Page Up and Page Down, or the nearest selectable
    /// one.
    fn page(&self, from: Option<usize>, step: isize) -> Option<usize> {
        let height = self.0.row_height.get();
        let viewport = match &*self.0.body.borrow() {
            Body::Listbox { list, .. } => f64::from(list.client_height()),
            Body::Select { .. } => 0.0,
        };
        let rows = match height > 0.0 {
            true => (viewport / height).floor().max(1.0) as isize,
            false => 1,
        };
        let last = self.len() as isize - 1;
        let start = match from {
            Some(index) => (index as isize + step * rows).clamp(0, last),
            None if step > 0 => 0,
            None => last,
        };
        self.first_enabled(start, step)
            .or_else(|| self.first_enabled(start, -step))
    }

    /// The row whose text starts with what has been typed, as in a `<select>`: letters
    /// typed within a second add up, and one letter repeated goes through the rows
    /// starting with it.
    ///
    /// Rows from [`List::set_row_builder`] have no text until they are built.
    fn type_ahead(&self, event: &KeyboardEvent, current: Option<usize>) -> Option<usize> {
        let key = event.key();
        if key.chars().count() != 1
            || event.ctrl_key()
            || event.meta_key()
            || event.alt_key()
            || event.is_composing()
            || self.0.rows.is_built()
        {
            return None;
        }
        let now = event.time_stamp();
        let mut typed = self.0.typed.borrow_mut();
        if now - self.0.typed_at.get() > TYPE_AHEAD_MILLIS {
            typed.clear();
        }
        // 打ち始めの Space は文字ではない。
        if typed.is_empty() && key == " " {
            return None;
        }
        self.0.typed_at.set(now);
        typed.push_str(&key.to_lowercase());
        let needle = match typed.chars().next() {
            Some(first) if typed.chars().all(|letter| letter == first) => first.to_string(),
            _ => typed.clone(),
        };
        drop(typed);
        // 1 文字なら次の行から探す。続けて打っている間は、いまの行のままで合えばよい。
        let len = self.len();
        let start = match (current, needle.chars().count()) {
            (Some(index), 1) => index + 1,
            (Some(index), _) => index,
            (None, _) => 0,
        };
        (0..len)
            .map(|offset| (start + offset) % len)
            .find(|&index| {
                self.is_selectable(index)
                    && self
                        .0
                        .rows
                        .with_stored(index, |row| match &row.content {
                            ListRowContent::Item(item) => {
                                item.label.to_lowercase().starts_with(&needle)
                            }
                            ListRowContent::Custom(_) => false,
                        })
                        .unwrap_or(false)
            })
    }

    /// Looks for the next selectable row from `from` in the direction of `step`.
    fn step(&self, from: Option<usize>, step: isize) -> Option<usize> {
        let start = match from {
            Some(index) => index as isize + step,
            None if step > 0 => 0,
            None => self.len() as isize - 1,
        };
        self.first_enabled(start, step)
    }

    /// Returns the first selectable row going from `start` in the direction of `step`.
    fn first_enabled(&self, start: isize, step: isize) -> Option<usize> {
        let len = self.len();
        let mut at = start;
        while at >= 0 && (at as usize) < len {
            if self.is_selectable(at as usize) {
                return Some(at as usize);
            }
            at += step;
        }
        None
    }

    /// The selectable rows from `a` to `b`.
    fn range(&self, a: usize, b: usize) -> Vec<usize> {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        (lo..=hi.min(self.len().saturating_sub(1)))
            .filter(|&i| self.is_selectable(i))
            .collect()
    }

    /// Settles the result of a user action and notifies.
    fn commit(&self, indices: &[usize]) {
        let picked = self.normalize(indices);
        self.write_selection(&picked);
        self.reveal_active();
        self.0.handler.emit(&picked);
    }

    /// Brings the row the keyboard points at into the scroll area.
    fn reveal_active(&self) {
        let Some(active) = self.0.active.get() else {
            return;
        };
        self.reveal_row(active);
        self.sync_active_descendant();
    }

    /// Points `aria-activedescendant` at the row the keyboard is on, if it is built.
    ///
    /// The attribute must name an element in the document, so it is removed while the row
    /// is outside the window (scrolled away with the wheel, say). `active` stays, so the
    /// keys carry on from that row, and the attribute returns with the row.
    fn sync_active_descendant(&self) {
        let body = self.0.body.borrow();
        let Body::Listbox { list, .. } = &*body else {
            return;
        };
        let window = self.0.window.get();
        let active = self.0.active.get().filter(|&index| window.contains(index));
        let _ = match active {
            Some(index) => list.set_attribute("aria-activedescendant", &self.option_id(index)),
            None => list.remove_attribute("aria-activedescendant"),
        };
        // 選択は動かないことがあるので、いまの行はブラウザのフォーカスの輪で見せる。
        if let Body::Listbox { options, .. } = &*body {
            for (offset, option) in options.iter().enumerate() {
                let current = active == Some(window.start + offset);
                style(option, "outline", if current { "auto" } else { "none" });
                style(option, "outline-offset", "-1px");
            }
        }
    }

    /// Brings row `index` into the scroll area, building the rows around it.
    fn reveal_row(&self, index: usize) {
        if index >= self.len() {
            return;
        }
        let window = self.0.window.get();
        let list = match &*self.0.body.borrow() {
            // すべての行があるなら、高さのそろわない行でも実際の位置へ送れる。
            Body::Listbox { list, options, .. } if window.is_complete() => {
                if let Some(option) = options.get(index) {
                    reveal_within(list, option);
                }
                return;
            }
            Body::Listbox { list, .. } => list.clone(),
            Body::Select { select, .. } => {
                reveal_option(select, index);
                return;
            }
        };
        // 絞っている間は、まだ無い行へ送れるように位置を行の高さから求める。
        // 載せた直後はまだ測っていないので、先に測る。
        self.settle_row_height();
        let height = self.0.row_height.get();
        // 隠れていると位置を動かせず、測っていなければ行き先も分からない。
        if height <= 0.0 || list.client_height() <= 0 {
            self.0.pending_reveal.set(Some(index));
            return;
        }
        let top = index as f64 * height;
        reveal_range(&list, top, top + height);
        // スクロールの通知を待たずに、指した行を組み立てておく。
        self.update_window();
    }
}

/// Creates a `<li>` that fills in for the rows outside the window.
///
/// It only has a height and is hidden from screen readers.
fn spacer_item(document: &Document) -> Result<HtmlElement> {
    let item: HtmlElement = create(document, "li")?.unchecked_into();
    let _ = item.set_attribute("aria-hidden", "true");
    let _ = item.set_attribute("role", "presentation");
    set_spacer_height(&item, 0.0);
    Ok(item)
}

/// Writes the spacer's height. At 0 it is collapsed.
fn set_spacer_height(item: &HtmlElement, height: f64) {
    style(item, "display", if height > 0.0 { "block" } else { "none" });
    style(item, "height", &format!("{height}px"));
}

/// Whether the event was aimed at a field inside the row.
fn entry_was_targeted(row: &HtmlElement, event: &Event) -> bool {
    let Some(target) = event.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
        return false;
    };
    let Ok(Some(entry)) = target.closest(TEXT_ENTRIES) else {
        return false;
    };
    row.contains(Some(entry.as_ref()))
}

/// Which row the clicked `<option>` is. `None` for clicks on the `<select>`'s empty
/// space.
fn clicked_option_index(event: &Event) -> Option<usize> {
    let target = event.target()?.dyn_into::<Element>().ok()?;
    let option = target.closest("option").ok()??;
    let index = option.dyn_into::<HtmlOptionElement>().ok()?.index();
    usize::try_from(index).ok()
}

/// Makes `row` visible inside the scroll area `container`.
///
/// If it is visible nothing moves; if it sticks out above, the top is aligned, and
/// if below, the bottom. The position is computed from the difference of the drawn
/// rectangles, so it does not depend on where the `offsetParent` is.
pub(crate) fn reveal_within(container: &HtmlElement, row: &HtmlElement) {
    let view = container.get_bounding_client_rect();
    let rect = row.get_bounding_client_rect();
    let top = rect.top() - view.top() + f64::from(container.scroll_top());
    reveal_range(container, top, top + rect.height());
}

/// Moves `container`'s vertical scroll to where `top..bottom` (content coordinates)
/// is visible.
pub(crate) fn reveal_range(container: &HtmlElement, top: f64, bottom: f64) {
    let scroll_top = f64::from(container.scroll_top());
    let visible = f64::from(container.client_height());
    if top < scroll_top {
        container.set_scroll_top(top.floor() as i32);
    } else if bottom > scroll_top + visible {
        container.set_scroll_top((bottom - visible).ceil() as i32);
    }
}

/// Brings row `index` of a `<select size>` into view.
///
/// WebKit rounds a `<select>`'s scroll position down to a row boundary, so scrolling
/// to the position computed from the rectangle leaves the last row half hidden. Row
/// heights are uniform, so the first row to show is decided in whole rows.
fn reveal_option(select: &web_sys::HtmlSelectElement, index: usize) {
    let count = select.length();
    if count == 0 || index >= count as usize {
        return;
    }
    // 行の高さは `<option>` から測る (全体の高さを行数で割ると、上下の
    // 余白のぶん小さくなり、切り下げで 1 行ずれる)。
    let measured = select
        .options()
        .item(index as u32)
        .map(|option| option.get_bounding_client_rect().height())
        .unwrap_or(0.0);
    let height = if measured > 0.0 {
        measured
    } else {
        f64::from(select.scroll_height()) / f64::from(count)
    };
    if height <= 0.0 {
        return;
    }
    let visible_rows = (f64::from(select.client_height()) / height)
        .floor()
        .max(1.0) as usize;
    let first = (f64::from(select.scroll_top()) / height).ceil() as usize;
    let target = if index < first {
        index
    } else if index >= first + visible_rows {
        index + 1 - visible_rows
    } else {
        return;
    };
    select.set_scroll_top((target as f64 * height).round() as i32);
}
