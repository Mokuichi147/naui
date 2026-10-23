//! A table (rows of a `GtkListBox` laid out horizontally).
//!
//! GTK4's `GtkColumnView` combines a `GtkListItemFactory` and a `GListModel` and
//! expects row data to be handed over as GObjects. naui's widgets take the form
//! "pass a sequence of values and the display changes", so it is built on the same
//! `GtkListBox` as [`crate::List`].
//!
//! | Part | Built from |
//! | --- | --- |
//! | Frame | A vertical `GtkBox` with the `frame` style class |
//! | Headings | A horizontal `GtkBox` + `GtkLabel`s, with a `GtkSeparator` below |
//! | Body | `GtkScrolledWindow` + `GtkListBox`. Rows are horizontal `GtkBox` + `GtkLabel` |
//! | Column widths | A `GtkSizeGroup` per column holding the heading and every row's cell, to align them |
//!
//! Column widths cannot be changed by dragging (unlike `NSTableView`, `GtkListBox`
//! has no mechanism for it).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::pango;
use gtk::prelude::*;
use naui_core::{
    keeps_hidden_selection, keeps_row_window, row_window, Align, Result, RowWindow,
    SelectionGesture, SelectionMode, SortOrder, TableColumn, TableRow, ROW_WINDOW_OVERSCAN,
};

use crate::bin::SizeBin;
use crate::callback::{SelectionNotifier, SortNotifier};
use crate::list::ActivationHandler;
use crate::widgets::{impl_widget, without_signal, Widget};

/// The spacing between cells.
const CELL_SPACING: i32 = 12;
/// The left and right margins of headings and rows.
const SIDE_MARGIN: i32 = 10;
/// The top and bottom margins of headings and rows.
const VERTICAL_MARGIN: i32 = 6;

/// Maps to GTK's text alignment. `Fill` means nothing for text, so it is treated
/// the same as left.
fn xalign(align: Align) -> f32 {
    match align {
        Align::Center => 0.5,
        Align::End => 1.0,
        Align::Start | Align::Fill => 0.0,
    }
}

/// The character indicating the sort direction. Appended after the heading text.
fn sort_arrow(order: Option<SortOrder>) -> &'static str {
    match order {
        Some(SortOrder::Ascending) => " ▲",
        Some(SortOrder::Descending) => " ▼",
        None => "",
    }
}

/// Creates the label for one cell. The column's width setting is applied here too.
fn cell_label(text: &str, column: &TableColumn, uniform: bool) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(xalign(column.align));
    // 列より長い文字は折り返さず、末尾を省略する。
    label.set_ellipsize(pango::EllipsizeMode::End);
    // 行を絞っている表では、セルの文字の長さで列幅を決めない。決めてしまうと、
    // スクロールで組み立て直すたびに列の幅が動いてしまう。列の幅は見出しと
    // 余りの分け合いだけで決まればよいので、自然な幅を 1 文字ぶんに抑える。
    if uniform && column.width.is_none() {
        label.set_max_width_chars(1);
    }
    apply_column_width(&label, column);
    label
}

/// Applies the column's width setting to the cell's content.
fn apply_column_width(widget: &impl IsA<gtk::Widget>, column: &TableColumn) {
    let widget = widget.as_ref();
    match column.width {
        // 幅の指定がある列は、その幅のまま。余りは受け取らない。
        Some(width) => {
            widget.set_size_request(width as i32, -1);
            widget.set_hexpand(false);
        }
        // 指定が無い列だけで、余った幅を分け合う。
        None => widget.set_hexpand(true),
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
/// # use naui_gtk::{Table, TableCells};
/// # fn fill(table: &Table, cities: Vec<String>) {
/// table.set_row_builder(cities.len(), move |index| {
///     Ok(TableCells::new().text(&cities[index]).text("13,960,000"))
/// });
/// # }
/// ```
pub struct TableCells {
    cells: Vec<CellContent>,
    selectable: bool,
    /// For text-only rows whose `enabled` is `false`. Makes the whole row inoperable.
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
    /// When a button or text field is clicked directly, that control receives the
    /// click, so this is not called.
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
        let mut stored = self.rows.borrow_mut();
        stored.clear();
        stored.extend_from_slice(rows);
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

struct TableInner {
    /// The widget visible from outside: a container stacking the headings and the body
    /// vertically.
    native: gtk::Box,
    /// The heading row. Its content is rebuilt whenever the columns change.
    header: gtk::Box,
    list: gtk::ListBox,
    /// `GtkListBox` does not scroll by itself, so it is placed in a scroll area.
    scroller: gtk::ScrolledWindow,
    /// A container stacking the spacers and the list vertically. The scroll area's
    /// content.
    _content: gtk::Box,
    /// Fills in for the rows outside the window, keeping the scroll bar length.
    top_spacer: gtk::Box,
    bottom_spacer: gtk::Box,
    bin: SizeBin,
    columns: RefCell<Vec<TableColumn>>,
    rows: Rc<RowsState>,
    /// The range of rows currently built. With many rows, only those around the screen.
    window: Cell<RowWindow>,
    /// Whether rows are being built right now. Prevents nested rebuilds.
    rebuilding: Cell<bool>,
    /// How the selection was changed (modifier keys), as inferred from the last input.
    gesture: Cell<SelectionGesture>,
    /// The contents of the built rows, in order from `window.start`.
    realized: RefCell<Vec<TableCells>>,
    /// The selected rows (ascending). **Rows outside the window are included**, so this
    /// is the source of truth, not the `GtkListBox`.
    selected: RefCell<Vec<usize>>,
    /// The height of one row (logical pixels). Either measured or set with
    /// [`Table::set_row_height`].
    row_height: Cell<f64>,
    /// The row height decided by the app. If absent, it is measured from a built row.
    fixed_row_height: Cell<Option<i32>>,
    /// A `GtkSizeGroup` per column. Recreated on every build.
    groups: RefCell<Vec<gtk::SizeGroup>>,
    /// The labels placed in the headings. Used to rewrite the sort indicator. In
    /// clickable columns they are the button's content.
    header_labels: RefCell<Vec<gtk::Label>>,
    /// The heading-side counterpart put into the `GtkSizeGroup` that aligns column
    /// widths. A button in clickable columns, otherwise a label.
    header_slots: RefCell<Vec<gtk::Widget>>,
    /// The current sort (column and direction).
    sort: Cell<Option<(usize, SortOrder)>>,
    on_select: SelectionNotifier,
    on_sort: SortNotifier,
    handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// A table with column headings. It scrolls by itself.
///
/// Its height is not determined by its contents, so set it with
/// [`Table::set_sizing`].
#[derive(Clone)]
pub struct Table(Rc<TableInner>);
impl_widget!(Table);

impl Table {
    pub(crate) fn new() -> Self {
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);

        let scroller = gtk::ScrolledWindow::new();
        scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        // 枠は外側の入れ物が持つので、こちらには付けない。
        scroller.set_has_frame(false);
        scroller.set_vexpand(true);
        // 行が多い表では、画面に出ている分だけを `GtkListBox` へ入れ、
        // 残りの行が占めるはずの高さを上下の詰め物が持つ。詰め物を一覧の
        // **外**に置くのは、`GtkListBox` の行として入れるとキーボード操作や
        // 行のインデックスに混ざってしまうため。
        let top_spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let bottom_spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&top_spacer);
        content.append(&list);
        content.append(&bottom_spacer);
        scroller.set_child(Some(&content));

        let header = gtk::Box::new(gtk::Orientation::Horizontal, CELL_SPACING);
        header.set_margin_start(SIDE_MARGIN);
        header.set_margin_end(SIDE_MARGIN);
        header.set_margin_top(VERTICAL_MARGIN);
        header.set_margin_bottom(VERTICAL_MARGIN);

        let native = gtk::Box::new(gtk::Orientation::Vertical, 0);
        // 一覧の枠。`GtkScrolledWindow` の枠と同じ見た目になる標準クラス。
        native.add_css_class("frame");
        native.append(&header);
        native.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        native.append(&scroller);

        let bin = SizeBin::wrap(&native);
        let inner = Rc::new(TableInner {
            native,
            header,
            list,
            scroller,
            _content: content,
            top_spacer,
            bottom_spacer,
            bin,
            columns: RefCell::new(Vec::new()),
            rows: Rc::new(RowsState::default()),
            window: Cell::new(RowWindow::default()),
            rebuilding: Cell::new(false),
            gesture: Cell::new(SelectionGesture::Unknown),
            realized: RefCell::new(Vec::new()),
            selected: RefCell::new(Vec::new()),
            row_height: Cell::new(0.0),
            fixed_row_height: Cell::new(None),
            groups: RefCell::new(Vec::new()),
            header_labels: RefCell::new(Vec::new()),
            header_slots: RefCell::new(Vec::new()),
            sort: Cell::new(None),
            on_select: SelectionNotifier::default(),
            on_sort: SortNotifier::default(),
            handler: RefCell::new(None),
        });
        // 選択の通知は常時つないでおき、プログラムから変えるときだけ止める。
        let id = {
            let weak = Rc::downgrade(&inner);
            inner.list.connect_selected_rows_changed(move |_| {
                if let Some(inner) = weak.upgrade() {
                    let table = Table(inner);
                    let selection = table.read_native_selection();
                    *table.0.selected.borrow_mut() = selection.clone();
                    table.0.on_select.emit(&selection);
                }
            })
        };
        *inner.handler.borrow_mut() = Some(id);

        let table = Self(inner);
        // 行のクリック (とキーボードの Enter / Space) は `row-activated` に出る。
        // セルの中のボタンや入力欄はそれ自身がクリックを受け取るので、
        // ここへは来ない。
        {
            let weak = Rc::downgrade(&table.0);
            table.0.list.connect_row_activated(move |_, row| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let Ok(offset) = usize::try_from(row.index()) else {
                    return;
                };
                // 通知の中から `set_rows` を呼べるように、借りたまま呼ばない。
                let activation = inner
                    .realized
                    .borrow()
                    .get(offset)
                    .map(|cells| cells.activation.clone());
                if let Some(activation) = activation {
                    activation.emit();
                }
            });
        }
        // 選択が変わる前に、そのとき押されていた修飾キーを覚えておく。
        // `selected-rows-changed` には修飾キーが乗らないが、窓の外の選択を
        // 残すかどうかの判断に要る (`keeps_hidden_selection`)。捕捉の段
        // (`Capture`) で見るので、一覧が選択を決めるより先に入る。
        {
            let controller = gtk::EventControllerLegacy::new();
            controller.set_propagation_phase(gtk::PropagationPhase::Capture);
            let weak = Rc::downgrade(&table.0);
            controller.connect_event(move |_, event| {
                if let Some(inner) = weak.upgrade() {
                    inner.gesture.set(gesture_of(event.modifier_state()));
                }
                glib::Propagation::Proceed
            });
            table.0.list.add_controller(controller);
        }
        // スクロールに合わせて、組み立てる範囲を動かす。
        {
            let weak = Rc::downgrade(&table.0);
            let update = move |_: &gtk::Adjustment| {
                if let Some(inner) = weak.upgrade() {
                    Table(inner).update_window();
                }
            };
            let adjustment = table.0.scroller.vadjustment();
            adjustment.connect_value_changed(update.clone());
            // 表そのものの大きさが変わったときも引き直す。
            adjustment.connect_page_size_notify(move |adjustment| update(adjustment));
        }
        table
    }

    /// Rebuilds the columns. Rows and selection stay; only the cells are rearranged.
    ///
    /// The sort setting also stays if that column can still be sorted.
    pub fn set_columns(&self, columns: &[TableColumn]) {
        // 行を作り直すと選択も落ちるので、覚えて書き戻す。
        let picked = self.selection();
        let sort = self
            .0
            .sort
            .get()
            .filter(|&(column, _)| columns.get(column).is_some_and(|spec| spec.sortable));
        self.0.sort.set(sort);
        {
            let mut stored = self.0.columns.borrow_mut();
            stored.clear();
            stored.extend_from_slice(columns);
        }
        self.rebuild();
        without_signal(&self.0.list, &self.0.handler, || self.show(&picked));
    }

    /// The number of columns.
    pub fn column_count(&self) -> usize {
        self.0.columns.borrow().len()
    }

    /// Rebuilds the rows. Indices change meaning, so the selection is cleared.
    ///
    /// With many rows, `GtkListBoxRow`s are created only for the rows on screen (the
    /// spacers above and below carry the height of the rest, so the scroll bar length
    /// still reflects all the rows).
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
    /// # use naui_gtk::{Table, TableCells};
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
    /// # use naui_gtk::{Table, TableCells};
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
        let picked = self.selection();
        self.build_window();
        without_signal(&self.0.list, &self.0.handler, || self.show(&picked));
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
            .set((height > 0.0).then_some(height as i32));
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
        self.0.selected.borrow_mut().clear();
        let multiple = mode.is_multiple();
        self.0.list.set_selection_mode(if multiple {
            // `Multiple` は「⌘ / Ctrl や Shift を押しながら選ぶ」形。
            gtk::SelectionMode::Multiple
        } else {
            gtk::SelectionMode::Single
        });
        // `GtkListBox` は「1 クリックで確定」(既定) の間、クリックに付いている
        // Ctrl / Shift を読まない。複数選択ではこれを切る (`List` と同じ)。
        self.0.list.set_activate_on_single_click(!multiple);
        // GTK4 がモードの変更で選択を落とすとは限らないので、明示的に外す。
        without_signal(&self.0.list, &self.0.handler, || {
            self.0.list.unselect_all();
        });
    }

    pub fn selection_mode(&self) -> SelectionMode {
        match self.0.list.selection_mode() {
            gtk::SelectionMode::Multiple => SelectionMode::Multiple,
            _ => SelectionMode::Single,
        }
    }

    /// The topmost of the selected rows.
    pub fn selected(&self) -> Option<usize> {
        self.selection().first().copied()
    }

    /// The selected rows (ascending). Zero or one for single selection.
    ///
    /// Rows outside the window are included. `GtkListBox` knows only the rows that are
    /// built, so the remembered selection is returned.
    pub fn selection(&self) -> Vec<usize> {
        self.0.selected.borrow().clone()
    }

    /// Scrolls until row `index` is visible. The selection does not change.
    ///
    /// If it is already visible nothing moves; otherwise it is aligned to the nearer
    /// edge (the top if it is above, the bottom if it is below). Does nothing if out of
    /// range.
    ///
    /// Even while only some rows are built, the position is computed from the row
    /// height, so it can scroll to rows not yet built (the destination row is built on
    /// the spot).
    pub fn scroll_to_row(&self, index: usize) {
        if index >= self.len() {
            return;
        }
        let height = self.0.row_height.get().max(1.0);
        let top = index as f64 * height;
        crate::list::reveal_range(&self.0.scroller, top, top + height);
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
        without_signal(&self.0.list, &self.0.handler, || self.show(&picked));
    }

    /// Clears the whole selection without notifying.
    pub fn clear_selection(&self) {
        without_signal(&self.0.list, &self.0.handler, || self.show(&[]));
    }

    /// Selects one row through the same path as a user's selection (notifies).
    pub fn select(&self, index: usize) {
        self.select_many(&[index]);
    }

    /// Replaces the selection through the same path as a user's selection (notifies).
    pub fn select_many(&self, indices: &[usize]) {
        let picked = self.normalize(indices);
        without_signal(&self.0.list, &self.0.handler, || self.show(&picked));
        self.0.on_select.emit(&picked);
    }

    /// Called with the indices of the selected rows every time the selection changes.
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.on_select.set(f);
    }

    /// Called with the clicked column and direction when a heading is clicked and the
    /// sort setting changes.
    ///
    /// Only columns with [`TableColumn::sortable`] set can be clicked. **Sorting the
    /// rows is the app's job**: when notified, pass the sorted rows again with
    /// [`Table::set_rows`] (`set_rows` clears the selection, so reselect if needed).
    pub fn on_sort(&self, f: impl FnMut(usize, SortOrder) + 'static) {
        self.0.on_sort.set(f);
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

    /// The inner `GtkListBox`. Public as a backend-specific escape hatch.
    pub fn native_list_box(&self) -> gtk::ListBox {
        self.0.list.clone()
    }

    // ------------------------------------------------------------ 組み立て

    /// Rebuilds the headings and rows from the current columns and rows.
    ///
    /// The per-column `GtkSizeGroup`s are recreated every time. Holding on to old cells
    /// would keep the widths of rows that are gone.
    fn rebuild(&self) {
        let columns = self.0.columns.borrow();
        while let Some(child) = self.0.header.first_child() {
            self.0.header.remove(&child);
        }
        let mut header_labels = Vec::with_capacity(columns.len());
        let mut header_slots: Vec<gtk::Widget> = Vec::with_capacity(columns.len());
        for (index, column) in columns.iter().enumerate() {
            let label = cell_label(&column.title, column, false);
            // 見出しは、行の文字より小さく淡くする (`List` の補助と同じ扱い)。
            crate::list::apply_caption(&label);
            match column.sortable {
                // 並べ替えられる列は、見出しそのものをボタンにする。
                // 幅をそろえる `GtkSizeGroup` の相手もボタンへ移す。
                true => {
                    let button = gtk::Button::new();
                    // 地色と枠を出さない、見出し向けの標準クラス。
                    button.add_css_class("flat");
                    button.set_hexpand(label.hexpands());
                    // 幅の指定と `GtkSizeGroup` の相手は、外側のボタンへ移す。
                    // 列の幅は「見出しの外形」で決まってほしいため。
                    if column.width.is_some() {
                        button.set_size_request(label.width_request(), -1);
                        label.set_size_request(-1, -1);
                    }
                    button.set_child(Some(&label));
                    header_slots.push(button.clone().upcast());
                    button.connect_clicked({
                        let weak = Rc::downgrade(&self.0);
                        move |_| {
                            if let Some(inner) = weak.upgrade() {
                                Table(inner).on_header_activated(index);
                            }
                        }
                    });
                    self.0.header.append(&button);
                }
                false => {
                    header_slots.push(label.clone().upcast());
                    self.0.header.append(&label);
                }
            }
            header_labels.push(label);
        }
        *self.0.header_labels.borrow_mut() = header_labels;
        *self.0.header_slots.borrow_mut() = header_slots;
        // 見出しを作り直したので、前の列の `GtkSizeGroup` は捨てる
        // (新しい見出しはまだどの組にも入っていない)。
        self.0.groups.borrow_mut().clear();
        drop(columns);
        self.build_window();
        self.apply_sort();
    }

    // ------------------------------------------------------- 行を絞る窓

    /// Rebuilds the rows and moves the window back to the start. The selection is
    /// cleared too.
    fn reset_rows(&self) {
        self.0.selected.borrow_mut().clear();
        self.0.window.set(self.compute_window(ROW_WINDOW_OVERSCAN));
        self.build_window();
        // 1 行目の高さが分かったら、その高さで窓を引き直す。
        if self.measure_row_height() {
            self.update_window();
        }
    }

    /// Rebuilds the `GtkListBox` rows for just the current window.
    fn build_window(&self) {
        // 組み立ての途中でアプリのコードが動くので、そこから呼ばれても
        // 二重に作り直さない。
        if self.0.rebuilding.replace(true) {
            return;
        }
        self.build_window_once();
        self.0.rebuilding.set(false);
    }

    fn build_window_once(&self) {
        let window = self.0.window.get();
        let columns = self.0.columns.borrow().clone();
        let uniform = !window.is_complete();
        let fixed_height = self.0.fixed_row_height.get();
        // 列ごとの `GtkSizeGroup` は組み立てのたびに作り直す。消えた行の
        // セルを持ったままだと、その幅が残ってしまうため。見出しは
        // 作り直さないので、古い組から外してから新しい組へ入れる。
        let slots = self.0.header_slots.borrow().clone();
        let groups: Vec<gtk::SizeGroup> = columns
            .iter()
            .map(|_| gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal))
            .collect();
        for (index, slot) in slots.iter().enumerate() {
            if let Some(group) = self.0.groups.borrow().get(index) {
                group.remove_widget(slot);
            }
            if let Some(group) = groups.get(index) {
                group.add_widget(slot);
            }
        }
        *self.0.groups.borrow_mut() = groups.clone();

        let mut realized = Vec::with_capacity(window.len());
        without_signal(&self.0.list, &self.0.handler, || {
            while let Some(row) = self.0.list.first_child() {
                self.0.list.remove(&row);
            }
            for index in window.indices() {
                let Some(cells) = self.0.rows.cells(index) else {
                    continue;
                };
                let row = build_row(&cells, &columns, &groups, uniform);
                if let Some(height) = fixed_height {
                    row.set_size_request(-1, height);
                }
                self.0.list.append(&row);
                realized.push(cells);
            }
            self.0.list.unselect_all();
        });
        *self.0.realized.borrow_mut() = realized;

        let height = self.0.row_height.get();
        set_spacer_height(&self.0.top_spacer, window.leading(height));
        set_spacer_height(&self.0.bottom_spacer, window.trailing(height));

        // 覚えている選択を、組み立て直した行へ写す。
        let picked = self.selection();
        without_signal(&self.0.list, &self.0.handler, || self.show(&picked));
    }

    /// Computes the range of rows to build from the current scroll position.
    fn compute_window(&self, overscan: usize) -> RowWindow {
        let adjustment = self.0.scroller.vadjustment();
        // 詰め物が窓の外の行と同じ高さを持つので、スクロール位置は
        // 「全行を作ったとき」と同じ座標になる。
        row_window(
            self.len(),
            adjustment.value(),
            adjustment.page_size(),
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
        self.build_window();
        self.measure_row_height();
    }

    /// Measures the height of one row from the built rows. `true` if it changed.
    ///
    /// Row height is decided by the content, so it is not known until a row is built.
    /// Only once it is known can the rows outside the window be filled in correctly.
    fn measure_row_height(&self) -> bool {
        if self.0.fixed_row_height.get().is_some() {
            return false;
        }
        let Some(row) = self.0.list.row_at_index(0) else {
            return false;
        };
        let measured = row.measure(gtk::Orientation::Vertical, -1).1 as f64;
        if measured <= 0.0 || (self.0.row_height.get() - measured).abs() < 0.5 {
            return false;
        }
        self.0.row_height.set(measured);
        let window = self.0.window.get();
        set_spacer_height(&self.0.top_spacer, window.leading(measured));
        set_spacer_height(&self.0.bottom_spacer, window.trailing(measured));
        true
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
        self.0.on_sort.emit(next.0, next.1);
    }

    /// Writes the sort setting into the headings.
    fn apply_sort(&self) {
        let sort = self.0.sort.get();
        let columns = self.0.columns.borrow();
        for (index, label) in self.0.header_labels.borrow().iter().enumerate() {
            let order = sort.filter(|&(column, _)| column == index).map(|(_, o)| o);
            let title = columns.get(index).map(|c| c.title.as_str()).unwrap_or("");
            label.set_text(&format!("{title}{}", sort_arrow(order)));
        }
    }

    /// Normalizes the requested selection into a form meaningful for this table.
    fn normalize(&self, indices: &[usize]) -> Vec<usize> {
        self.selection_mode()
            .normalize_by(indices, |index| self.is_selectable(index))
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

    /// Remembers the selection and applies it to the built rows.
    ///
    /// Rows outside the window have no `GtkListBoxRow`, so it cannot be applied to
    /// them. When the window moves and rows are rebuilt, it is applied again from the
    /// remembered selection.
    fn show(&self, picked: &[usize]) {
        *self.0.selected.borrow_mut() = picked.to_vec();
        let window = self.0.window.get();
        self.0.list.unselect_all();
        for &index in picked {
            if !window.contains(index) {
                continue;
            }
            let offset = (index - window.start) as i32;
            if let Some(row) = self.0.list.row_at_index(offset) {
                self.0.list.select_row(Some(&row));
            }
        }
    }

    /// Reads the selection the user changed. How rows outside the window are treated is
    /// decided here too.
    fn read_native_selection(&self) -> Vec<usize> {
        let window = self.0.window.get();
        let mut picked: Vec<usize> = self
            .0
            .list
            .selected_rows()
            .iter()
            .map(|row| window.start + row.index().max(0) as usize)
            .collect();
        picked.sort_unstable();
        if window.is_complete() {
            return picked;
        }
        // 組み立ててある行の選択しか届かないので、窓の外の選択を残すかどうかを
        // 修飾キーから決める (`keeps_hidden_selection`)。
        let previous = self.0.selected.borrow().clone();
        let inside: Vec<usize> = previous
            .iter()
            .copied()
            .filter(|&index| window.contains(index))
            .collect();
        if !keeps_hidden_selection(&inside, &picked, self.0.gesture.get()) {
            return picked;
        }
        let outside = previous.iter().copied().filter(|&i| !window.contains(i));
        picked.extend(outside);
        picked.sort_unstable();
        picked.dedup();
        picked
    }
}

/// Builds one row. Cells are in the same order as the columns.
fn build_row(
    cells: &TableCells,
    columns: &[TableColumn],
    groups: &[gtk::SizeGroup],
    uniform: bool,
) -> gtk::ListBoxRow {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, CELL_SPACING);
    content.set_margin_top(VERTICAL_MARGIN);
    content.set_margin_bottom(VERTICAL_MARGIN);
    content.set_margin_start(SIDE_MARGIN);
    content.set_margin_end(SIDE_MARGIN);

    for (index, column) in columns.iter().enumerate() {
        match cells.content(index) {
            Some(CellContent::Widget(widget)) => {
                let bin = widget.size_bin();
                bin.fill_parent();
                apply_column_width(&bin, column);
                // 行を絞っているときは、ウィジェットの幅で列幅を決めない。
                // 行ごとに中身の幅が違うと、スクロールで列が動いてしまう
                // (文字のセルを 1 文字ぶんに抑えているのと同じ扱い)。
                if uniform && column.width.is_none() {
                    bin.limit_natural_width();
                }
                groups[index].add_widget(&bin);
                content.append(&bin);
            }
            // 列より短い行は、足りない分が空のセルになる。
            Some(CellContent::Text(text)) => {
                let label = cell_label(text, column, uniform);
                // 見出しと同じ列を 1 つの `GtkSizeGroup` に入れると、幅がそろう。
                groups[index].add_widget(&label);
                content.append(&label);
            }
            None => {
                let label = cell_label("", column, uniform);
                groups[index].add_widget(&label);
                content.append(&label);
            }
        }
    }

    let native = gtk::ListBoxRow::new();
    native.set_child(Some(&content));
    let selectable = cells.is_selectable();
    native.set_selectable(selectable);
    // 選べない行でも、クリックを受けたい行はある (行内のボタンだけを使う行)。
    native.set_activatable(selectable || cells.activation.is_some());
    // 文字だけの行で `enabled` が `false` のときは、行ごと操作できなくする。
    native.set_sensitive(!cells.dimmed);
    native
}

/// Infers how the selection is being changed from the modifier keys held.
fn gesture_of(state: gtk::gdk::ModifierType) -> SelectionGesture {
    if state.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
        // Shift は起点からの範囲。選び直しなので、窓の外の選択は落とす。
        return SelectionGesture::Extend;
    }
    if state.intersects(gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::META_MASK) {
        return SelectionGesture::Toggle;
    }
    SelectionGesture::Plain
}

/// Gives the spacer the height of the rows outside the window.
fn set_spacer_height(spacer: &gtk::Box, height: f64) {
    let height = height.max(0.0).min(i32::MAX as f64) as i32;
    spacer.set_size_request(-1, height);
    spacer.set_visible(height > 0);
}
