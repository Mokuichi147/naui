//! Tables (DOM).
//!
//! HTML's `<table>` is used as is. Column headings are `<th scope="col">`, rows
//! `<tr>` and cells `<td>`. Column widths are held by the `<col>`s of a
//! `<colgroup>`, and **width computation is left to the browser's table layout**.
//!
//! A plain `<table>` has no mechanism for "selecting rows", so that is the only
//! thing naui adds.
//!
//! | Part | Built from |
//! | --- | --- |
//! | Role | `role="grid"` + `aria-multiselectable` |
//! | Selection | `<tr aria-selected>` and system colors (`SelectedItem` / `Highlight`) |
//! | Interaction | Row clicks (⌘ / Ctrl / Shift), arrows, Home / End, Space |
//!
//! The frame, selection and disabled colors all use the browser's system colors as
//! is. naui decides no colors.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{
    keeps_row_window, row_window, Align, Result, RowWindow, SelectionMode, SortOrder, TableColumn,
    TableRow, ROW_WINDOW_OVERSCAN,
};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlElement, HtmlTableElement, KeyboardEvent, MouseEvent};

use crate::list::{control_was_targeted, ActivationHandler};
use crate::widgets::{create, impl_widget, Listener, Widget};

thread_local! {
    /// A per-table running number for referring to rows from
    /// `aria-activedescendant`.
    static NEXT_ID: Cell<u32> = const { Cell::new(0) };
}

fn next_table_id() -> u32 {
    NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    })
}

fn style(element: &HtmlElement, property: &str, value: &str) {
    let _ = element.style().set_property(property, value);
}

/// Maps to CSS text alignment. `Fill` means nothing for text, so it is treated the
/// same as left.
fn text_align(align: Align) -> &'static str {
    match align {
        Align::Center => "center",
        Align::End => "right",
        Align::Start | Align::Fill => "left",
    }
}

/// The handler notified when the selection changes.
///
/// To use the same shape for single and multiple selection, the selected rows are
/// passed in ascending order. The closure is taken out only for the duration of the
/// call, so operating on the table from inside the callback does not double-borrow.
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

/// The handler notified when the sort changes.
#[derive(Clone, Default)]
struct SortHandler(Rc<RefCell<Option<Box<dyn FnMut(usize, SortOrder)>>>>);

impl SortHandler {
    fn set(&self, f: impl FnMut(usize, SortOrder) + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, column: usize, order: SortOrder) {
        let Some(mut f) = self.0.borrow_mut().take() else {
            return;
        };
        f(column, order);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
    }
}

/// The content of one cell.
enum CellContent {
    Text(String),
    Widget(Box<dyn Widget>),
}

impl Clone for CellContent {
    fn clone(&self) -> Self {
        match self {
            Self::Text(text) => Self::Text(text.clone()),
            Self::Widget(content) => Self::Widget(content.boxed_clone()),
        }
    }
}

/// The contents of one row to put in a table.
///
/// [`TableRow`] is a convenience API for tables that need only strings. To put
/// buttons, check boxes or icons in cells, build the contents with `Grid` / `Stack`
/// and line them up in this type. Rows are returned from
/// [`Table::set_row_builder`].
///
/// ```no_run
/// # use naui_web::{Table, TableCells};
/// # fn fill(table: &Table, cities: Vec<String>) {
/// table.set_row_builder(cities.len(), move |index| {
///     Ok(TableCells::new().text(&cities[index]).text("13,960,000"))
/// });
/// # }
/// ```
pub struct TableCells {
    cells: Vec<CellContent>,
    selectable: bool,
    /// For text-only rows whose `enabled` is `false`. Dims the text.
    dimmed: bool,
    activation: ActivationHandler,
}

impl Clone for TableCells {
    fn clone(&self) -> Self {
        Self {
            cells: self.cells.clone(),
            selectable: self.selectable,
            dimmed: self.dimmed,
            activation: self.activation.clone(),
        }
    }
}

impl Default for TableCells {
    fn default() -> Self {
        Self::new()
    }
}

impl TableCells {
    /// Creates a row with no cells.
    pub fn new() -> Self {
        Self {
            cells: Vec::new(),
            selectable: true,
            dimmed: false,
            activation: ActivationHandler::default(),
        }
    }

    /// Adds one text cell. Alignment follows the column's setting.
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.cells.push(CellContent::Text(text.into()));
        self
    }

    /// Adds one widget cell.
    pub fn cell(mut self, content: &dyn Widget) -> Self {
        self.cells.push(CellContent::Widget(content.boxed_clone()));
        self
    }

    /// Whether the whole row can be selected (it can by default).
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    pub fn is_selectable(&self) -> bool {
        self.selectable
    }

    /// The number of columns.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// What to do when a cell or empty space in the row is clicked.
    ///
    /// It is not called when a control such as a button or text field is clicked
    /// directly.
    pub fn on_activate(&self, f: impl FnMut() + 'static) {
        self.activation.set(f);
    }

    /// Creates one from a text-only row. `enabled` becomes "whether it can be
    /// selected" as is.
    fn from_row(row: &TableRow) -> Self {
        Self {
            cells: row.cells.iter().cloned().map(CellContent::Text).collect(),
            selectable: row.enabled,
            dimmed: !row.enabled,
            activation: ActivationHandler::default(),
        }
    }

    fn content(&self, index: usize) -> Option<&CellContent> {
        self.cells.get(index)
    }
}

/// The closure that builds rows. Taken out only for the duration of the call.
/// The slot for the closure that builds rows.
type RowBuildCell = Rc<RefCell<Option<Box<dyn FnMut(usize) -> Result<TableCells>>>>>;

/// The slot for the function answering "can this row be selected?"
/// ([`Table::set_row_selectable`]).
type RowSelectableCell = RefCell<Option<Rc<dyn Fn(usize) -> bool>>>;

/// The closure that builds rows.
///
/// It is taken out only for the duration of the call, so operating on the table
/// from inside the build does not double-borrow.
#[derive(Clone, Default)]
struct RowBuilder(RowBuildCell);

impl RowBuilder {
    fn set(&self, f: impl FnMut(usize) -> Result<TableCells> + 'static) {
        *self.0.borrow_mut() = Some(Box::new(f));
    }

    fn clear(&self) {
        *self.0.borrow_mut() = None;
    }

    fn is_set(&self) -> bool {
        self.0.borrow().is_some()
    }

    fn build(&self, index: usize) -> Option<Result<TableCells>> {
        let mut f = self.0.borrow_mut().take()?;
        let cells = f(index);
        let mut slot = self.0.borrow_mut();
        if slot.is_none() {
            *slot = Some(f);
        }
        Some(cells)
    }
}

/// Where rows come from: either text-only rows ([`Table::set_rows`]) or rows built
/// when they become visible ([`Table::set_row_builder`]).
#[derive(Default)]
struct RowsState {
    rows: RefCell<Vec<TableRow>>,
    builder: RowBuilder,
    /// The function that answers "can it be selected?" without building the row
    /// ([`Table::set_row_selectable`]).
    selectable: RowSelectableCell,
    count: Cell<usize>,
}

impl RowsState {
    fn len(&self) -> usize {
        self.count.get()
    }

    fn set_rows(&self, rows: &[TableRow]) {
        self.builder.clear();
        *self.rows.borrow_mut() = rows.to_vec();
        self.count.set(rows.len());
    }

    fn set_builder(&self, count: usize, f: impl FnMut(usize) -> Result<TableCells> + 'static) {
        self.rows.borrow_mut().clear();
        self.builder.set(f);
        self.count.set(count);
    }

    /// The contents of that row. `None` if there is none.
    fn cells(&self, index: usize) -> Option<TableCells> {
        if index >= self.count.get() {
            return None;
        }
        if !self.builder.is_set() {
            return self.rows.borrow().get(index).map(TableCells::from_row);
        }
        // 組み立てに失敗した行は、列だけそろえた空の行にする。
        Some(self.builder.build(index)?.unwrap_or_default())
    }

    fn set_selectable(&self, f: impl Fn(usize) -> bool + 'static) {
        *self.selectable.borrow_mut() = Some(Rc::new(f));
    }

    /// Whether the row can be selected, if that can be answered without building it.
    ///
    /// No borrow is held during the call (the app may touch the table from inside it).
    fn selectable_hint(&self, index: usize) -> Option<bool> {
        let f = self.selectable.borrow().clone()?;
        Some(f(index))
    }

    /// For text-only rows, whether the row can be selected. `None` for built rows.
    fn text_row_selectable(&self, index: usize) -> Option<bool> {
        match self.builder.is_set() {
            true => None,
            false => Some(self.rows.borrow().get(index).is_some_and(|row| row.enabled)),
        }
    }
}

/// The character indicating the sort direction. HTML has no "sorted" look, so
/// screen readers get `aria-sort` and the eyes get an arrow.
fn sort_arrow(order: Option<SortOrder>) -> &'static str {
    match order {
        Some(SortOrder::Ascending) => " ▲",
        Some(SortOrder::Descending) => " ▼",
        None => "",
    }
}

struct TableInner {
    /// The frame visible from outside. This is what scrolls.
    root: HtmlElement,
    document: Document,
    id: u32,
    table: HtmlElement,
    /// The `<colgroup>` holding the column widths. Its content is rebuilt whenever the
    /// columns change.
    colgroup: HtmlElement,
    head: HtmlElement,
    body: HtmlElement,
    /// The headings' `<th>`s. The sort indicator is written here.
    header_cells: RefCell<Vec<HtmlElement>>,
    /// The `<button>`s placed inside the headings. Only clickable columns have one.
    sort_buttons: RefCell<Vec<Option<HtmlElement>>>,
    /// The subscriptions to heading clicks. Replaced whenever the columns are rebuilt.
    sort_listeners: RefCell<Vec<Listener>>,
    /// The current sort (column and direction).
    sort: Cell<Option<(usize, SortOrder)>>,
    sort_handler: SortHandler,
    columns: RefCell<Vec<TableColumn>>,
    rows: Rc<RowsState>,
    /// The range of rows currently built. With many rows, only those around the screen.
    window: Cell<RowWindow>,
    /// The contents of the built rows, in the same order as `row_elements`.
    realized: RefCell<Vec<TableCells>>,
    /// The height of one row (logical pixels). Either measured or set with
    /// [`Table::set_row_height`].
    row_height: Cell<f64>,
    /// The row height decided by the app. If absent, it is measured from a built row.
    fixed_row_height: Cell<Option<f64>>,
    /// A `<tr>` filling in for the rows outside the window, keeping the scroll bar
    /// length.
    top_spacer: HtmlElement,
    bottom_spacer: HtmlElement,
    /// The `<tr>`s of the built rows, in order from `window.start`.
    row_elements: RefCell<Vec<HtmlElement>>,
    mode: Cell<SelectionMode>,
    /// The selected rows (ascending). This is the source of truth, not the DOM.
    selected: RefCell<Vec<usize>>,
    /// The row the keyboard is pointing at now.
    active: Cell<Option<usize>>,
    /// The anchor of a Shift range selection.
    anchor: Cell<Option<usize>>,
    handler: SelectionHandler,
    /// The click subscription per row. Replaced whenever the rows are rebuilt.
    row_listeners: RefCell<Vec<Listener>>,
    /// The subscription for key operations on the whole table. The `<table>` is never
    /// rebuilt, so it is attached only once.
    _keys: RefCell<Option<Listener>>,
    /// The scroll subscription. Used to move the window.
    _scroll: RefCell<Option<Listener>>,
}

/// A table with column headings (`<table>`).
///
/// Its height is decided by its contents, so to fix it regardless of the number of
/// rows, specify it with `set_sizing`. Whatever overflows scrolls inside the frame.
#[derive(Clone)]
pub struct Table(Rc<TableInner>);
impl_widget!(Table, root);

impl Table {
    pub(crate) fn new(doc: &Document) -> Result<Self> {
        let root: HtmlElement = create(doc, "div")?.unchecked_into();
        style(&root, "overflow", "auto");
        style(&root, "min-height", "0");
        // 枠と地の色は、ブラウザが入力欄に使うシステム色に任せる。
        // 線は `GrayText` にする。Safari (WebKit) の `ButtonBorder` は明暗
        // どちらの配色でも白に解決されるため、明るい配色では線が消え、
        // 暗い配色では強く出すぎる (`SplitView` の仕切りと同じ判断)。
        style(&root, "border", "1px solid");
        style(&root, "border-color", "GrayText");
        style(&root, "background-color", "Field");
        style(&root, "color", "FieldText");
        // 行の `offsetTop` がこの要素を基準になるようにする
        // (`reveal_active` がスクロール位置を求めるのに使う)。
        style(&root, "position", "relative");

        let table: HtmlElement = create(doc, "table")?.unchecked_into();
        let _ = table.set_attribute("role", "grid");
        // キーボードで入れるようにする。中の行は `aria-activedescendant` で指す。
        let _ = table.set_attribute("tabindex", "0");
        style(&table, "width", "100%");
        // **枠は重ねない (`collapse` にしない)。** 重ねた枠はセルではなく表が
        // 描くので、見出しを `position: sticky` で留めても枠が付いてこず、
        // ブラウザによっては見出しの地色ごと描かれない (行が透けて見える)。
        // 区切り線はセルの枠として引き、間隔は 0 にして重ねたときと同じ細さに
        // する。
        style(&table, "border-collapse", "separate");
        style(&table, "border-spacing", "0");
        // 列の幅を `<col>` の指定どおりにする。指定の無い列は余りを分け合う。
        style(&table, "table-layout", "fixed");

        let colgroup: HtmlElement = create(doc, "colgroup")?.unchecked_into();
        let top_spacer = spacer_row(doc)?;
        let bottom_spacer = spacer_row(doc)?;
        let head: HtmlElement = create(doc, "thead")?.unchecked_into();
        let body: HtmlElement = create(doc, "tbody")?.unchecked_into();
        let _ = table.append_child(&colgroup);
        let _ = table.append_child(&head);
        let _ = table.append_child(&body);
        let _ = root.append_child(&table);

        let this = Self(Rc::new(TableInner {
            root,
            document: doc.clone(),
            id: next_table_id(),
            table,
            colgroup,
            head,
            body,
            header_cells: RefCell::new(Vec::new()),
            sort_buttons: RefCell::new(Vec::new()),
            sort_listeners: RefCell::new(Vec::new()),
            sort: Cell::new(None),
            sort_handler: SortHandler::default(),
            columns: RefCell::new(Vec::new()),
            rows: Rc::new(RowsState::default()),
            window: Cell::new(RowWindow::default()),
            realized: RefCell::new(Vec::new()),
            row_height: Cell::new(0.0),
            fixed_row_height: Cell::new(None),
            top_spacer,
            bottom_spacer,
            row_elements: RefCell::new(Vec::new()),
            mode: Cell::new(SelectionMode::Single),
            selected: RefCell::new(Vec::new()),
            active: Cell::new(None),
            anchor: Cell::new(None),
            handler: SelectionHandler::default(),
            row_listeners: RefCell::new(Vec::new()),
            _keys: RefCell::new(None),
            _scroll: RefCell::new(None),
        }));
        this.apply_mode();

        let keys = Listener::attach_event(this.0.table.as_ref(), "keydown", {
            let weak = Rc::downgrade(&this.0);
            move |event| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                if let Some(key) = event.dyn_ref::<KeyboardEvent>() {
                    if Table(inner).on_key(key) {
                        event.prevent_default();
                    }
                }
            }
        })?;
        *this.0._keys.borrow_mut() = Some(keys);

        // 行が多い表では、スクロールに合わせて組み立てる範囲を動かす。
        let scroll = Listener::attach(this.0.root.as_ref(), "scroll", {
            let weak = Rc::downgrade(&this.0);
            move || {
                if let Some(inner) = weak.upgrade() {
                    Table(inner).update_window();
                }
            }
        })?;
        *this.0._scroll.borrow_mut() = Some(scroll);
        Ok(this)
    }

    /// Rebuilds the columns. Rows and selection stay; only the cells are rearranged.
    ///
    /// The sort setting also stays if that column can still be sorted.
    pub fn set_columns(&self, columns: &[TableColumn]) {
        *self.0.columns.borrow_mut() = columns.to_vec();
        let _ = self.build_columns();
        // 見出しを作り直したので、指標を貼り直す (押せなくなった列なら外れる)。
        let sort = self
            .0
            .sort
            .get()
            .filter(|&(column, _)| columns.get(column).is_some_and(|spec| spec.sortable));
        self.0.sort.set(sort);
        self.apply_sort();
        // セルの数と揃えが変わるので、行も組み直す。
        let _ = self.build_window();
        let picked = self.selection();
        self.write_selection(&picked);
    }

    /// The number of columns.
    pub fn column_count(&self) -> usize {
        self.0.columns.borrow().len()
    }

    /// Rebuilds the rows. Indices change meaning, so the selection is cleared.
    ///
    /// With many rows, `<tr>`s are created only for the rows on screen (the spacers
    /// above and below carry the height of the rest, so the scroll bar still reflects
    /// all the rows).
    pub fn set_rows(&self, rows: &[TableRow]) {
        self.0.rows.set_rows(rows);
        self.reset_rows();
    }

    /// Passes rows whose cells can hold widgets, in a form that **builds each row when
    /// it becomes visible**.
    ///
    /// `count` is the number of rows, and `build` receives an index from 0 to
    /// `count - 1` and returns that row's contents ([`TableCells`]). It is called only
    /// for the rows on screen (and a few before and after), so opening stays fast even
    /// with hundreds of thousands of rows.
    ///
    /// ```no_run
    /// # use naui_web::{Table, TableCells};
    /// # fn fill(table: &Table, cities: Vec<String>) {
    /// table.set_row_builder(cities.len(), move |index| {
    ///     Ok(TableCells::new().text(&cities[index]))
    /// });
    /// # }
    /// ```
    pub fn set_row_builder(
        &self,
        count: usize,
        build: impl FnMut(usize) -> Result<TableCells> + 'static,
    ) {
        self.0.rows.set_builder(count, build);
        self.reset_rows();
    }

    /// Passes a function that answers whether a row can be selected **without building
    /// the row**.
    ///
    /// For rows built with [`Table::set_row_builder`], naui does not know whether rows
    /// not yet built can be selected. With this function, [`Table::set_selection`] can
    /// keep its promise of "removing rows that cannot be selected" even for rows off
    /// screen. Without it, built rows follow [`TableCells::selectable`] and rows not
    /// yet built are treated as selectable.
    ///
    /// Its answers must match the [`TableCells::selectable`] returned by `build`.
    ///
    /// ```no_run
    /// # use naui_web::{Table, TableCells};
    /// # fn fill(table: &Table, done: std::rc::Rc<Vec<bool>>) {
    /// let rows = done.clone();
    /// table.set_row_selectable(move |index| !rows[index]);
    /// # }
    /// ```
    pub fn set_row_selectable(&self, selectable: impl Fn(usize) -> bool + 'static) {
        self.0.rows.set_selectable(selectable);
    }

    /// Rebuilds the visible rows. The row count and selection stay.
    pub fn refresh(&self) {
        let _ = self.build_window();
        let picked = self.selection();
        self.write_selection(&picked);
        // 中身が変わって行の高さが動いていれば、窓を引き直す。
        if self.measure_row_height() {
            self.update_window();
        }
    }

    /// Sets the row height in logical pixels. Passing 0 or less measures it from a
    /// built row.
    ///
    /// Every row gets the same height. Rows off screen are filled in as "row count ×
    /// this height", so if row heights are not uniform the scroll bar length will be
    /// slightly off from reality.
    pub fn set_row_height(&self, height: f64) {
        self.0
            .fixed_row_height
            .set((height > 0.0).then_some(height));
        // 指定を外したときは、組み立て直した行から測り直す。
        self.0.row_height.set(height.max(0.0));
        self.refresh();
    }

    /// The row height (logical pixels). 0 if no row has been built yet.
    pub fn row_height(&self) -> f64 {
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
    ///
    /// Even while only some rows are built (with more than
    /// [`ROW_WINDOW_THRESHOLD`](naui_core::ROW_WINDOW_THRESHOLD) rows), the destination
    /// row is built.
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
    /// selection only the first remains ([`SelectionMode::normalize_by`]).
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

    /// Called with the clicked column and direction when a heading is clicked and the
    /// sort setting changes.
    ///
    /// Only columns with [`TableColumn::sortable`] set can be clicked. **Sorting the
    /// rows is the app's job**: when notified, pass the sorted rows again with
    /// [`Table::set_rows`] (`set_rows` clears the selection, so reselect if needed).
    pub fn on_sort(&self, f: impl FnMut(usize, SortOrder) + 'static) {
        self.0.sort_handler.set(f);
    }

    /// The current sort setting (column and direction). `None` if no heading has been
    /// clicked.
    pub fn sort(&self) -> Option<(usize, SortOrder)> {
        self.0.sort.get()
    }

    /// Replaces the sort setting without notifying. Only the heading's indicator
    /// changes.
    ///
    /// Pointing at a column that cannot be sorted, or one out of range, clears the
    /// setting.
    pub fn set_sort(&self, sort: Option<(usize, SortOrder)>) {
        let sort = sort.filter(|&(column, _)| {
            self.0
                .columns
                .borrow()
                .get(column)
                .is_some_and(|spec| spec.sortable)
        });
        self.0.sort.set(sort);
        self.apply_sort();
    }

    /// The inner `<table>`. Public as a backend-specific escape hatch.
    ///
    /// The frame (the scrolling `<div>`) can be obtained from
    /// [`Widget::native_element`].
    pub fn native_table(&self) -> HtmlTableElement {
        self.0.table.clone().unchecked_into()
    }

    // ------------------------------------------------------------ 組み立て

    /// Rebuilds the `<colgroup>` and `<thead>` from the current column definitions.
    fn build_columns(&self) -> Result<()> {
        clear(&self.0.colgroup);
        clear(&self.0.head);
        let columns = self.0.columns.borrow();

        for column in columns.iter() {
            let col: HtmlElement = create(&self.0.document, "col")?.unchecked_into();
            // 幅の指定が無い列は、余りをブラウザが分け合わせる。
            if let Some(width) = column.width {
                style(&col, "width", &format!("{width}px"));
            }
            let _ = self.0.colgroup.append_child(&col);
        }

        let row: HtmlElement = create(&self.0.document, "tr")?.unchecked_into();
        let mut cells = Vec::with_capacity(columns.len());
        let mut buttons = Vec::with_capacity(columns.len());
        let mut listeners = Vec::new();
        for (index, column) in columns.iter().enumerate() {
            let cell: HtmlElement = create(&self.0.document, "th")?.unchecked_into();
            let _ = cell.set_attribute("scope", "col");
            style(&cell, "text-align", text_align(column.align));
            style(&cell, "padding", "4px 8px");
            // スクロールしても見出しが残るようにする。行の中のボタンや入力欄は
            // 自分で重なりの層を作るので、見出しにも層を与えて上に置く
            // (`z-index` が無いと、それらが見出しの上へ描かれてしまう)。
            style(&cell, "position", "sticky");
            style(&cell, "top", "0");
            style(&cell, "z-index", "1");
            // **見出しの地は透けてはいけない。** 行がこの下を流れるため。
            // Safari (WebKit) の暗い配色では、入力欄の地色 `Field` が
            // 半透明 (白 24.7%) に解決されるので、そのまま塗ると行が透ける。
            // 不透明な `Canvas` (ページの地色) を敷いた上へ同じ `Field` を
            // 重ねて、表の地と同じ見た目のまま不透明にする。
            style(&cell, "background-color", "Canvas");
            style(&cell, "background-image", "linear-gradient(Field, Field)");
            style(&cell, "border-bottom", "1px solid");
            style(&cell, "border-color", "GrayText");

            // 並べ替えられる列は、見出しそのものを `<button>` にする。
            // キーボードでも押せて、読み上げにも「押せる」と伝わる。
            let button = match column.sortable {
                false => {
                    cell.set_text_content(Some(&column.title));
                    None
                }
                true => {
                    let button: HtmlElement = create(&self.0.document, "button")?.unchecked_into();
                    button.set_text_content(Some(&column.title));
                    // 見出しの中では、ボタンらしい枠や地色は出さない。
                    style(&button, "all", "unset");
                    style(&button, "display", "block");
                    style(&button, "width", "100%");
                    style(&button, "cursor", "pointer");
                    style(&button, "text-align", text_align(column.align));
                    listeners.push(Listener::attach(button.as_ref(), "click", {
                        let weak = Rc::downgrade(&self.0);
                        move || {
                            if let Some(inner) = weak.upgrade() {
                                Table(inner).on_header_activated(index);
                            }
                        }
                    })?);
                    let _ = cell.append_child(&button);
                    Some(button)
                }
            };

            let _ = row.append_child(&cell);
            cells.push(cell);
            buttons.push(button);
        }
        *self.0.header_cells.borrow_mut() = cells;
        *self.0.sort_buttons.borrow_mut() = buttons;
        *self.0.sort_listeners.borrow_mut() = listeners;

        self.0
            .head
            .append_child(&row)
            .map(|_| ())
            .map_err(|e| crate::to_error("テーブルの見出しの組み立て", e))
    }

    /// Rebuilds the `<tbody>` for just the current window.
    ///
    /// Rows outside the window get no `<tr>`; the spacers above and below carry their
    /// height. The table's height and the scroll bar length stay those of all the rows.
    fn build_window(&self) -> Result<()> {
        clear(&self.0.body);
        self.0.row_listeners.borrow_mut().clear();
        self.0.realized.borrow_mut().clear();
        // 組み立ての中でアプリのコードが動くので、列の借用は持ち越さない。
        let columns = self.0.columns.borrow().clone();
        let window = self.0.window.get();
        let height = self.0.row_height.get();

        // 読み上げには、窓ではなく表そのものの行数を伝える。
        let _ = self
            .0
            .table
            .set_attribute("aria-rowcount", &(window.count + 1).to_string());
        set_spacer_height(&self.0.top_spacer, window.leading(height), columns.len());
        set_spacer_height(
            &self.0.bottom_spacer,
            window.trailing(height),
            columns.len(),
        );
        let _ = self.0.body.append_child(&self.0.top_spacer);

        let mut elements = Vec::with_capacity(window.len());
        let mut listeners = Vec::with_capacity(window.len());
        let mut realized = Vec::with_capacity(window.len());

        for index in window.indices() {
            let Some(cells) = self.0.rows.cells(index) else {
                continue;
            };
            let element: HtmlElement = create(&self.0.document, "tr")?.unchecked_into();
            let _ = element.set_attribute("id", &self.row_id(index));
            let _ = element.set_attribute("aria-selected", "false");
            // 見出しの行が 1 行目なので、データの 1 行目は 2 になる。
            let _ = element.set_attribute("aria-rowindex", &(index + 2).to_string());
            if let Some(fixed) = self.0.fixed_row_height.get() {
                style(&element, "height", &format!("{fixed}px"));
            }

            for (column_index, column) in columns.iter().enumerate() {
                let cell: HtmlElement = create(&self.0.document, "td")?.unchecked_into();
                style(&cell, "text-align", text_align(column.align));
                style(&cell, "padding", "4px 8px");
                // 行の区切りだけを引く。縦線まで引くと表としては強すぎる。
                // 下側へ引くのは、見出しの枠と重なって 2 本にならないため
                // (枠を重ねない `separate` にしているため)。
                style(&cell, "border-bottom", "1px solid");
                style(&cell, "border-color", "GrayText");
                match cells.content(column_index) {
                    Some(CellContent::Widget(content)) => {
                        let _ = cell.append_child(&content.native_element());
                    }
                    // 列より短い行は、足りない分が空のセルになる。
                    Some(CellContent::Text(text)) => {
                        cell.set_text_content(Some(text));
                        clip_text(&cell);
                    }
                    None => {
                        cell.set_text_content(Some(""));
                        clip_text(&cell);
                    }
                }
                let _ = element.append_child(&cell);
            }

            if cells.dimmed {
                let _ = element.set_attribute("aria-disabled", "true");
                // 無効な文字にブラウザが使うシステム色。
                style(&element, "color", "GrayText");
            } else {
                let selectable = cells.is_selectable();
                // 選択不可でも activation とセル内の操作は有効なので、
                // 無効を表す aria-disabled や薄い文字色は付けない。
                listeners.push(Listener::attach_event(element.as_ref(), "click", {
                    let weak = Rc::downgrade(&self.0);
                    let element = element.clone();
                    move |event| {
                        let Some(inner) = weak.upgrade() else {
                            return;
                        };
                        let table = Table(inner);
                        // セルの中のボタンや入力欄そのものを押したときは、
                        // 行の activation も選択も起こさない。押した先の
                        // コントロールだけが動く (ほかの環境では、その
                        // コントロールがクリックを受け取るので同じになる)。
                        if control_was_targeted(&element, &event) {
                            return;
                        }
                        table.activate_row(index);
                        if !selectable {
                            return;
                        }
                        let mouse = event.dyn_ref::<MouseEvent>();
                        let toggle = mouse.is_some_and(|e| e.meta_key() || e.ctrl_key());
                        let extend = mouse.is_some_and(|e| e.shift_key());
                        table.on_row_activated(index, toggle, extend);
                    }
                })?);
            }

            let _ = self.0.body.append_child(&element);
            elements.push(element);
            realized.push(cells);
        }

        let _ = self.0.body.append_child(&self.0.bottom_spacer);
        *self.0.row_elements.borrow_mut() = elements;
        *self.0.row_listeners.borrow_mut() = listeners;
        *self.0.realized.borrow_mut() = realized;
        self.paint_selection();
        Ok(())
    }

    /// Fires that row's activation.
    fn activate_row(&self, index: usize) {
        let window = self.0.window.get();
        let activation = self
            .0
            .realized
            .borrow()
            .get(index.wrapping_sub(window.start))
            .map(|cells| cells.activation.clone());
        if let Some(activation) = activation {
            activation.emit();
        }
    }

    // ------------------------------------------------------- 行を絞る窓

    /// Rebuilds the rows and moves the window back to the start. The selection is
    /// cleared too.
    fn reset_rows(&self) {
        self.0.selected.borrow_mut().clear();
        self.0.active.set(None);
        self.0.anchor.set(None);
        self.0.window.set(self.compute_window(ROW_WINDOW_OVERSCAN));
        let _ = self.build_window();
        // 1 行目の高さが分かったら、その高さで窓を引き直す。
        if self.measure_row_height() {
            self.update_window();
        }
    }

    /// Computes the range of rows to build from the current scroll position.
    fn compute_window(&self, overscan: usize) -> RowWindow {
        let head = self.0.head.offset_height() as f64;
        let viewport = self.0.root.client_height() as f64 - head;
        row_window(
            self.len(),
            self.0.root.scroll_top() as f64,
            viewport,
            self.0.row_height.get(),
            overscan,
        )
    }

    /// Moves the built range along with scrolling.
    ///
    /// Rebuilding for every row scrolled is heavy, so the window is reused until you
    /// have scrolled through half of the extra rows ([`ROW_WINDOW_OVERSCAN`]).
    fn update_window(&self) {
        let current = self.0.window.get();
        let needed = self.compute_window(ROW_WINDOW_OVERSCAN / 2);
        let next = self.compute_window(ROW_WINDOW_OVERSCAN);
        if next == current || keeps_row_window(current, needed, next) {
            return;
        }
        self.0.window.set(next);
        let _ = self.build_window();
        self.measure_row_height();
    }

    /// Measures the height of one row from the built rows. `true` if it changed.
    ///
    /// The browser decides row heights, so they are not known until a row is built. Only
    /// once it is known can the rows outside the window be filled in correctly.
    fn measure_row_height(&self) -> bool {
        if self.0.fixed_row_height.get().is_some() {
            return false;
        }
        let measured = self
            .0
            .row_elements
            .borrow()
            .first()
            .map(|row| row.offset_height() as f64)
            .unwrap_or(0.0);
        if measured <= 0.0 || (self.0.row_height.get() - measured).abs() < 0.5 {
            return false;
        }
        self.0.row_height.set(measured);
        // 詰め物の高さは行の高さから決まるので、引き直す。
        let window = self.0.window.get();
        let columns = self.column_count();
        set_spacer_height(&self.0.top_spacer, window.leading(measured), columns);
        set_spacer_height(&self.0.bottom_spacer, window.trailing(measured), columns);
        true
    }

    /// The top of the row within the table (logical pixels). Can be computed even for
    /// rows outside the window.
    fn row_top(&self, index: usize) -> f64 {
        index as f64 * self.0.row_height.get()
    }

    /// When a heading is clicked. The same column reverses direction; a different
    /// column starts ascending.
    fn on_header_activated(&self, index: usize) {
        let next = match self.0.sort.get() {
            Some((column, order)) if column == index => (index, order.reversed()),
            _ => (index, SortOrder::Ascending),
        };
        self.0.sort.set(Some(next));
        self.apply_sort();
        self.0.sort_handler.emit(next.0, next.1);
    }

    /// Writes the sort setting into the headings.
    fn apply_sort(&self) {
        let sort = self.0.sort.get();
        let columns = self.0.columns.borrow();
        let cells = self.0.header_cells.borrow();
        let buttons = self.0.sort_buttons.borrow();
        for (index, cell) in cells.iter().enumerate() {
            let order = sort.filter(|&(column, _)| column == index).map(|(_, o)| o);
            let value = match order {
                Some(SortOrder::Ascending) => "ascending",
                Some(SortOrder::Descending) => "descending",
                // 押せる列は「まだ並べ替えていない」、押せない列は指定なし。
                None if columns.get(index).is_some_and(|spec| spec.sortable) => "none",
                None => {
                    let _ = cell.remove_attribute("aria-sort");
                    continue;
                }
            };
            let _ = cell.set_attribute("aria-sort", value);
            if let Some(Some(button)) = buttons.get(index) {
                let title = columns.get(index).map(|c| c.title.as_str()).unwrap_or("");
                button.set_text_content(Some(&format!("{title}{}", sort_arrow(order))));
            }
        }
    }

    fn row_id(&self, index: usize) -> String {
        format!("naui-table-{}-row-{index}", self.0.id)
    }

    /// Applies the single / multiple setting to the DOM.
    fn apply_mode(&self) {
        let multiple = self.0.mode.get().is_multiple();
        let _ = self.0.table.set_attribute(
            "aria-multiselectable",
            if multiple { "true" } else { "false" },
        );
    }

    // --------------------------------------------------------------- 選択

    /// Normalizes the requested selection into a form meaningful for this table.
    fn normalize(&self, indices: &[usize]) -> Vec<usize> {
        self.0
            .mode
            .get()
            .normalize_by(indices, |i| self.is_selectable(i))
    }

    /// Whether the row can be selected.
    ///
    /// For built rows, rows not yet built are assumed "selectable" (it cannot be known
    /// without building them).
    fn is_selectable(&self, index: usize) -> bool {
        if index >= self.len() {
            return false;
        }
        if let Some(enabled) = self.0.rows.text_row_selectable(index) {
            return enabled;
        }
        // 組み立てずに答えられるなら、そちらが先 (画面の外の行にも効く)。
        if let Some(selectable) = self.0.rows.selectable_hint(index) {
            return selectable;
        }
        let window = self.0.window.get();
        match window.contains(index) {
            true => self
                .0
                .realized
                .borrow()
                .get(index - window.start)
                .is_none_or(TableCells::is_selectable),
            false => true,
        }
    }

    /// Remembers the selection and writes it into the rows as is (no notification).
    fn write_selection(&self, indices: &[usize]) {
        *self.0.selected.borrow_mut() = indices.to_vec();
        self.paint_selection();
    }

    /// Writes the remembered selection into the built rows.
    ///
    /// Rows outside the window have no `<tr>`, so only the visible ones can be written.
    /// The source of truth is [`TableInner::selected`], so when the window moves and
    /// rows are rebuilt, it is written again then.
    fn paint_selection(&self) {
        let indices = self.0.selected.borrow().clone();
        let start = self.0.window.get().start;
        for (offset, element) in self.0.row_elements.borrow().iter().enumerate() {
            let index = start + offset;
            let picked = indices.contains(&index);
            let _ = element.set_attribute("aria-selected", if picked { "true" } else { "false" });
            if picked {
                // 選択の色はブラウザのシステム色をそのまま使う。
                // 新しい名前に対応していれば、後の指定が勝つ。
                style(element, "background-color", "Highlight");
                style(element, "background-color", "SelectedItem");
                style(element, "color", "HighlightText");
                style(element, "color", "SelectedItemText");
            } else {
                style(element, "background-color", "");
                style(element, "color", "");
                if element.has_attribute("aria-disabled") {
                    style(element, "color", "GrayText");
                }
            }
        }
    }

    /// When a row is clicked.
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
        let target = match event.key().as_str() {
            "ArrowDown" => self.step(current, 1),
            "ArrowUp" => self.step(current, -1),
            "Home" => self.first_enabled(0, 1),
            "End" => self.first_enabled(len as isize - 1, -1),
            // 複数選択では Space で、いま指している行を入れたり外したりする。
            " " if self.0.mode.get().is_multiple() => {
                let Some(index) = current else {
                    return false;
                };
                self.on_row_activated(index, true, false);
                return true;
            }
            _ => return false,
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
        let _ = self
            .0
            .table
            .set_attribute("aria-activedescendant", &self.row_id(active));
        self.reveal_row(active);
    }

    /// Brings row `index` into the scroll area and builds the rows around it.
    fn reveal_row(&self, active: usize) {
        if active >= self.len() {
            return;
        }
        // 載せた直後は 1 行の高さをまだ測っていないので、位置を出す前に測る。
        self.measure_row_height();
        // 見出しはスクロールしても残る (`position: sticky`) ので、
        // その分だけ画面の上側は行に使えない。
        let head = self.0.head.offset_height() as f64;
        let height = self.0.row_height.get().max(1.0);
        let top = self.row_top(active);
        let bottom = top + height;
        let view_top = self.0.root.scroll_top() as f64;
        let view_bottom = view_top + self.0.root.client_height() as f64 - head;
        if top < view_top {
            self.0.root.set_scroll_top(top as i32);
        } else if bottom > view_bottom {
            self.0
                .root
                .set_scroll_top((bottom - (self.0.root.client_height() as f64 - head)) as i32);
        }
        // スクロールの通知を待たずに、指した行を組み立てておく。
        self.update_window();
    }
}

/// Creates a `<tr>` that fills in for the rows outside the window.
///
/// It only has a height and is hidden from screen readers. The table's height stays
/// that of all the rows, so the scroll bar's length and position are the same as if
/// every row had been created.
fn spacer_row(document: &Document) -> Result<HtmlElement> {
    let row: HtmlElement = create(document, "tr")?.unchecked_into();
    let _ = row.set_attribute("aria-hidden", "true");
    let _ = row.set_attribute("role", "presentation");
    let cell: HtmlElement = create(document, "td")?.unchecked_into();
    style(&cell, "padding", "0");
    style(&cell, "border", "0");
    let _ = row.append_child(&cell);
    Ok(row)
}

/// Writes the spacer's height. At 0 the row itself is collapsed.
fn set_spacer_height(row: &HtmlElement, height: f64, columns: usize) {
    let hidden = height <= 0.0;
    style(row, "display", if hidden { "none" } else { "" });
    style(row, "height", &format!("{height}px"));
    if let Some(cell) = row.first_element_child() {
        let _ = cell.set_attribute("colspan", &columns.max(1).to_string());
        if let Some(cell) = cell.dyn_ref::<HtmlElement>() {
            style(cell, "height", &format!("{height}px"));
        }
    }
}

/// Cuts text longer than the column at the column's width.
fn clip_text(cell: &HtmlElement) {
    style(cell, "overflow", "hidden");
    style(cell, "text-overflow", "ellipsis");
    style(cell, "white-space", "nowrap");
}

/// Removes all of the element's contents.
fn clear(element: &HtmlElement) {
    while let Some(child) = element.last_element_child() {
        let _ = element.remove_child(&child);
    }
}
