//! Tables (AppKit).
//!
//! A multi-column `NSTableView` placed in an `NSScrollView`. It is the same control
//! as lists ([`crate::List`]), but here an `NSTableColumn` is created per column and
//! headings are shown with an `NSTableHeaderView`. Column width adjustment, row
//! drawing, scrolling, multiple selection with ⌘ / Shift and keyboard navigation are
//! all done by AppKit.
//!
//! naui adds only three things — "return the cell's string", "report rows that
//! cannot be selected" and "pass selection changes to Rust closures" — and defines
//! one data source + delegate class for them.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use naui_core::{Align, Result, SelectionMode, SortOrder, TableColumn, TableRow};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSColor, NSControlTextEditingDelegate, NSLayoutConstraint, NSLayoutConstraintOrientation,
    NSLayoutPriorityDefaultHigh, NSLayoutPriorityDefaultLow, NSScrollView, NSTableCellView,
    NSTableColumn, NSTableColumnResizingOptions, NSTableHeaderView, NSTableView,
    NSTableViewColumnAutoresizingStyle, NSTableViewDataSource, NSTableViewDelegate,
    NSTableViewGridLineStyle, NSTableViewStyle, NSTextAlignment, NSTextField, NSView,
};
use objc2_foundation::{
    NSArray, NSIndexSet, NSInteger, NSMutableIndexSet, NSNotification, NSSize, NSSortDescriptor,
    NSString,
};

use crate::list::{selected_indices, ActivationHandler, SelectionHandler};
use crate::trampoline::ActionTarget;
use crate::widgets::Widget;

/// The prefix of column identifiers. The column index is appended.
const COLUMN_ID_PREFIX: &str = "naui.table.column.";
/// The padding left and right of a cell.
const CELL_PADDING: f64 = 4.0;
/// The padding above and below the text. The row height is "the label's height +
/// twice this".
const ROW_SPACING: f64 = 5.0;
/// The minimum width given to columns without a width. They cannot be narrower than
/// this.
const MIN_COLUMN_WIDTH: f64 = 40.0;

/// Extracts the column index from the identifier.
fn column_index(column: Option<&NSTableColumn>) -> Option<usize> {
    let column = column?;
    let identifier = column.identifier().to_string();
    identifier.strip_prefix(COLUMN_ID_PREFIX)?.parse().ok()
}

/// The handler notified when the sort changes.
///
/// The closure is taken out only for the duration of the call, so operating on the
/// same table from inside the callback does not double-borrow (the same shape as
/// [`SelectionHandler`]).
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
///
/// Both text-only rows ([`TableRow`]) and built rows ([`TableCells`]) are
/// normalized into this for display.
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
/// # use naui_macos::{TableCells, Widget};
/// # fn row(button: &dyn Widget) {
/// let cells = TableCells::new().text("Tokyo").cell(button).selectable(false);
/// cells.on_activate(|| println!("row clicked"));
/// # let _ = cells;
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
    ///
    /// The column's [`TableColumn::align`] is used to decide where in the column the
    /// cell is placed ([`Align::Fill`] stretches it to fill the column).
    pub fn cell(mut self, content: &dyn Widget) -> Self {
        self.cells.push(CellContent::Widget(content.boxed_clone()));
        self
    }

    /// Whether the whole row can be selected (it can by default).
    ///
    /// Set to `false` for rows where only the buttons or check boxes inside are
    /// operated.
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
    /// It is not called when a button, check box or text field is clicked directly, so
    /// the row's action and the inner control's action do not both fire (the same
    /// division as [`crate::ListRow`]).
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

/// The closure that builds rows.
///
/// It is taken out only for the duration of the call, so operating on the table
/// from inside the build does not double-borrow (the same shape as
/// [`SelectionHandler`]).
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
///
/// `NSTableView` calls `tableView:viewForTableColumn:row:` only for visible rows,
/// so however many rows there are, only those on screen are built. Built rows are
/// remembered until they leave the screen (to reuse them when building other columns
/// of the same row).
#[derive(Default)]
struct RowsState {
    rows: RefCell<Vec<TableRow>>,
    builder: RowBuilder,
    /// The function that answers "can it be selected?" without building the row
    /// ([`Table::set_row_selectable`]).
    selectable: RowSelectableCell,
    count: Cell<usize>,
    built: RefCell<HashMap<usize, TableCells>>,
}

impl RowsState {
    fn len(&self) -> usize {
        self.count.get()
    }

    fn set_rows(&self, rows: &[TableRow]) {
        self.builder.clear();
        self.built.borrow_mut().clear();
        *self.rows.borrow_mut() = rows.to_vec();
        self.count.set(rows.len());
    }

    fn set_builder(&self, count: usize, f: impl FnMut(usize) -> Result<TableCells> + 'static) {
        self.rows.borrow_mut().clear();
        self.built.borrow_mut().clear();
        self.builder.set(f);
        self.count.set(count);
    }

    /// Discards the built rows. They are rebuilt the next time they become visible.
    fn forget_built(&self) {
        self.built.borrow_mut().clear();
    }

    /// The contents of that row. `None` if there is none.
    fn cells(&self, index: usize) -> Option<TableCells> {
        if index >= self.count.get() {
            return None;
        }
        if !self.builder.is_set() {
            return self.rows.borrow().get(index).map(TableCells::from_row);
        }
        if let Some(cells) = self.built.borrow().get(&index) {
            return Some(cells.clone());
        }
        // 組み立てに失敗した行は、列だけそろえた空の行にする。
        let cells = self.builder.build(index)?.unwrap_or_default();
        self.built.borrow_mut().insert(index, cells.clone());
        Some(cells)
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

    /// Whether the row can be selected.
    ///
    /// For built rows, unless [`Table::set_row_selectable`] has been given, rows not
    /// yet built are assumed "selectable" (it cannot be known without building them).
    /// Rows that have appeared on screen follow their setting at that time.
    fn is_selectable(&self, index: usize) -> bool {
        if index >= self.count.get() {
            return false;
        }
        if !self.builder.is_set() {
            return self.rows.borrow().get(index).is_some_and(|row| row.enabled);
        }
        if let Some(selectable) = self.selectable_hint(index) {
            return selectable;
        }
        self.built
            .borrow()
            .get(&index)
            .map(TableCells::is_selectable)
            .unwrap_or(true)
    }

    /// The row's click handler.
    fn activation(&self, index: usize) -> Option<ActivationHandler> {
        self.built
            .borrow()
            .get(&index)
            .map(|cells| cells.activation.clone())
    }

    /// Forgets a row that left the screen, so built rows do not pile up without limit.
    fn forget_row(&self, index: usize) {
        if !self.builder.is_set() {
            return;
        }
        self.built.borrow_mut().remove(&index);
    }
}

/// Reads the table's sort setting as a column and direction.
fn sort_from(table: &NSTableView) -> Option<(usize, SortOrder)> {
    let descriptor = table.sortDescriptors().firstObject()?;
    let key = descriptor.key()?.to_string();
    let column = key.strip_prefix(COLUMN_ID_PREFIX)?.parse().ok()?;
    Some((
        column,
        match descriptor.ascending() {
            true => SortOrder::Ascending,
            false => SortOrder::Descending,
        },
    ))
}

/// Distributes the leftover width evenly to the columns without a width.
///
/// AppKit's `columnAutoresizingStyle` **cannot distribute everything when some
/// columns have a fixed width**. It tries to grow them proportionally, but columns
/// with `minWidth == maxWidth` cannot move, and their share is left over (leaving
/// blank space on the right of the table). naui decides the table's width, so it
/// decides the distribution too.
fn distribute_widths(table: &NSTableView, columns: &[TableColumn]) {
    let available = table.frame().size.width;
    if available <= 0.0 {
        return;
    }
    let flexible: Vec<usize> = columns
        .iter()
        .enumerate()
        .filter(|(_, column)| column.width.is_none())
        .map(|(index, _)| index)
        .collect();
    if flexible.is_empty() {
        return;
    }
    // AppKit は「列の幅の合計 + 列の間の余白」を表の幅として並べる。
    let fixed: f64 = columns.iter().filter_map(|column| column.width).sum();
    let spacing = table.intercellSpacing().width * columns.len() as f64;
    let each = ((available - fixed - spacing) / flexible.len() as f64).max(MIN_COLUMN_WIDTH);

    let native = table.tableColumns();
    for &index in &flexible {
        if index >= native.len() {
            continue;
        }
        let column = native.objectAtIndex(index);
        // 同じ幅を書き戻すと `setFrameSize:` が何度も呼ばれて回り続けるので、
        // 変わるときだけ書く。
        if (column.width() - each).abs() > 0.5 {
            column.setWidth(each);
        }
    }
}

/// The state kept for distributing widths. Shared with the handle.
struct TableViewState {
    columns: Rc<RefCell<Vec<TableColumn>>>,
}

define_class!(
    #[unsafe(super(NSTableView))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiTableView"]
    #[ivars = TableViewState]
    /// An `NSTableView` that redistributes the leftover to the columns every time its
    /// width changes.
    struct TableView;

    unsafe impl NSObjectProtocol for TableView {}

    impl TableView {
        #[unsafe(method(setFrameSize:))]
        fn set_frame_size(&self, size: NSSize) {
            let _: () = unsafe { msg_send![super(self), setFrameSize: size] };
            distribute_widths(self, &self.ivars().columns.borrow());
        }
    }
);

impl TableView {
    fn new(mtm: MainThreadMarker, state: TableViewState) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(state);
        unsafe { msg_send![super(this), init] }
    }
}

/// The state seen by the data source and delegate. Shared with the handle.
struct SourceState {
    columns: Rc<RefCell<Vec<TableColumn>>>,
    rows: Rc<RowsState>,
    handler: SelectionHandler,
    sort_handler: SortHandler,
    /// Stops notifications only while the selection is being changed from code, since
    /// AppKit calls the delegate even for `selectRowIndexes:`.
    silent: Rc<Cell<bool>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "NauiTableSource"]
    #[ivars = SourceState]
    struct TableSource;

    unsafe impl NSObjectProtocol for TableSource {}

    unsafe impl NSTableViewDataSource for TableSource {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows(&self, _table_view: &NSTableView) -> NSInteger {
            self.ivars().rows.len() as NSInteger
        }

        // 並べ替えの通知はデリゲートではなくデータソースへ来る。
        #[unsafe(method(tableView:sortDescriptorsDidChange:))]
        fn sort_descriptors_did_change(
            &self,
            table_view: &NSTableView,
            _old: &NSArray<NSSortDescriptor>,
        ) {
            // 指標 (▲▼) の描き替えとヘッダーの押し込みは AppKit が行う。
            // naui は「どの列を、どちら向きに」だけをアプリへ渡す。
            let state = self.ivars();
            if state.silent.get() {
                return;
            }
            let Some((column, order)) = sort_from(table_view) else {
                return;
            };
            state.sort_handler.emit(column, order);
        }
    }

    // NSTableViewDelegate は NSControlTextEditingDelegate を継承している。
    unsafe impl NSControlTextEditingDelegate for TableSource {}

    unsafe impl NSTableViewDelegate for TableSource {
        // Retained を返すので `method_id`。所有権の扱いは objc2 が面倒を見る。
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn view_for_row(
            &self,
            _table_view: &NSTableView,
            column: Option<&NSTableColumn>,
            row: NSInteger,
        ) -> Option<Retained<NSView>> {
            // `?` は define_class! の中では使えないので、組み立ては外の関数で行う。
            view_for_cell(self.ivars(), MainThreadMarker::from(self), column, row)
        }

        #[unsafe(method(tableView:shouldSelectRow:))]
        fn should_select_row(&self, _table_view: &NSTableView, row: NSInteger) -> bool {
            usize::try_from(row)
                .ok()
                .is_some_and(|row| self.ivars().rows.is_selectable(row))
        }

        // 行が画面から外れたら、その行のために組み立てたものを忘れる。
        //
        // **ここからテーブルへ問い合わせない。** このデリゲートは行の
        // 作り直しの途中でも呼ばれるので、`rowsInRect:` のような
        // 位置を尋ねる呼び出しを混ぜると、AppKit に「デリゲートの中で
        // 再入した」と警告される (将来はアサートになる)。外れた行の分だけを
        // 捨てれば足りる。次に見えたときは組み立て直される。
        #[unsafe(method(tableView:didRemoveRowView:forRow:))]
        fn did_remove_row_view(
            &self,
            _table_view: &NSTableView,
            _row_view: &NSView,
            row: NSInteger,
        ) {
            // 行が消されたときは -1 が来る。そのときは何もしない
            // (行の入れ替えでは `reloadData` 側がまとめて捨てている)。
            if let Ok(row) = usize::try_from(row) {
                self.ivars().rows.forget_row(row);
            }
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_did_change(&self, notification: &NSNotification) {
            // 行数は `rows` から見る。`set_rows` が `rows` を書き換えてから
            // `reloadData` を呼ぶまでの間はテーブルと食い違うが、その区間は
            // `without_notifying` で囲まれていてここへは来ない。
            let state = self.ivars();
            if state.silent.get() {
                return;
            }
            let Some(object) = notification.object() else {
                return;
            };
            let Ok(table) = object.downcast::<NSTableView>() else {
                return;
            };
            let indices = selected_indices(&table, state.rows.len());
            state.handler.emit(&indices);
        }
    }
);

/// Builds a cell's view from a column and row. `None` if either is missing.
fn view_for_cell(
    state: &SourceState,
    mtm: MainThreadMarker,
    column: Option<&NSTableColumn>,
    row: NSInteger,
) -> Option<Retained<NSView>> {
    let index = column_index(column)?;
    let align = state
        .columns
        .borrow()
        .get(index)
        .map(|column| column.align)
        .unwrap_or(Align::Start);
    let cells = state.rows.cells(usize::try_from(row).ok()?)?;
    Some(match cells.content(index) {
        Some(CellContent::Widget(content)) => widget_cell_view(mtm, &**content, align),
        // 列が足りない行は空のセルになる (`TableRow::cell` と同じ扱い)。
        Some(CellContent::Text(text)) => cell_view(mtm, text, align, !cells.dimmed),
        None => cell_view(mtm, "", align, !cells.dimmed),
    })
}

impl TableSource {
    fn new(mtm: MainThreadMarker, state: SourceState) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(state);
        unsafe { msg_send![super(this), init] }
    }
}

/// The row height. Decided by measuring one label laid out in the system font.
///
/// **It is not automatic (`usesAutomaticRowHeights`).** When columns are added or
/// removed, the constraints AppKit places between the row view and cells are
/// activated while still pointing at cells that left the row with the previous
/// column, causing an exception ("no common ancestor" from
/// `-[NSTableRowData setColumnHidden:atColumnIndex:]`). Every row of a table can
/// be the same height, so the height is decided here.
fn default_row_height(mtm: MainThreadMarker) -> f64 {
    let probe = NSTextField::labelWithString(&NSString::from_str("Ag"), mtm);
    probe.fittingSize().height + ROW_SPACING * 2.0
}

/// Maps to AppKit's text alignment. `Fill` means nothing for text, so it is treated
/// the same as left.
fn text_alignment(align: Align) -> NSTextAlignment {
    match align {
        Align::Center => NSTextAlignment::Center,
        Align::End => NSTextAlignment::Right,
        Align::Start | Align::Fill => NSTextAlignment::Left,
    }
}

/// Creates the view for one cell.
///
/// Unlike list rows ([`crate::list::cell_view`]), both the left and right edges are
/// **connected to the column's width**. `NSTextField` decides text alignment within
/// its own width, so right and center alignment only work when it fills the column.
/// Text longer than the column is shrunk by AppKit by lowering its compression
/// resistance.
///
/// Vertically it is only centered. The row height is fixed by [`ROW_HEIGHT`], and
/// connecting the top and bottom to the edges would make them compete for height.
fn cell_view(mtm: MainThreadMarker, text: &str, align: Align, enabled: bool) -> Retained<NSView> {
    let cell = NSTableCellView::new(mtm);
    let field = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    field.setAlignment(text_alignment(align));
    let color = if enabled {
        NSColor::labelColor()
    } else {
        // 選べない行は、AppKit が無効なコントロールに使う色で描く。
        NSColor::disabledControlTextColor()
    };
    field.setTextColor(Some(&color));
    field.setTranslatesAutoresizingMaskIntoConstraints(false);
    // 列より長い文字列があっても、列幅につないだ制約のほうを通す。
    field.setContentCompressionResistancePriority_forOrientation(
        NSLayoutPriorityDefaultLow - 1.0,
        NSLayoutConstraintOrientation::Horizontal,
    );
    cell.addSubview(&field);
    // `textField` に入れておくと、行の背景色に合わせた文字色や
    // アクセシビリティの扱いを NSTableCellView が面倒を見る。
    unsafe { cell.setTextField(Some(&field)) };

    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
        field
            .leadingAnchor()
            .constraintEqualToAnchor_constant(&cell.leadingAnchor(), CELL_PADDING),
        cell.trailingAnchor()
            .constraintEqualToAnchor_constant(&field.trailingAnchor(), CELL_PADDING),
        field
            .centerYAnchor()
            .constraintEqualToAnchor(&cell.centerYAnchor()),
    ]));

    let view: &NSView = cell.as_ref();
    view.retain()
}

/// Places a widget cell in the column.
///
/// The constraints keeping it within the column are **not required**. If required
/// constraints clashed when the content is wider than the column, AppKit would break
/// the layout. Where in the column it is placed is decided by
/// [`TableColumn::align`]; only [`Align::Fill`] stretches it to fill the column.
fn widget_cell_view(mtm: MainThreadMarker, content: &dyn Widget, align: Align) -> Retained<NSView> {
    let cell = NSTableCellView::new(mtm);
    let view = content.native_view();
    view.setTranslatesAutoresizingMaskIntoConstraints(false);
    cell.addSubview(&view);

    let inside = [
        view.leadingAnchor()
            .constraintGreaterThanOrEqualToAnchor_constant(&cell.leadingAnchor(), CELL_PADDING),
        cell.trailingAnchor()
            .constraintGreaterThanOrEqualToAnchor_constant(&view.trailingAnchor(), CELL_PADDING),
    ];
    for constraint in &inside {
        constraint.setPriority(NSLayoutPriorityDefaultHigh);
    }
    let mut constraints: Vec<Retained<NSLayoutConstraint>> = inside.into();
    // 縦は中央ぞろえだけにする。行の高さは表が決めているため。
    constraints.push(
        view.centerYAnchor()
            .constraintEqualToAnchor(&cell.centerYAnchor()),
    );
    let leading = || {
        view.leadingAnchor()
            .constraintEqualToAnchor_constant(&cell.leadingAnchor(), CELL_PADDING)
    };
    let trailing = || {
        cell.trailingAnchor()
            .constraintEqualToAnchor_constant(&view.trailingAnchor(), CELL_PADDING)
    };
    match align {
        Align::Start => constraints.push(leading()),
        Align::Center => constraints.push(
            view.centerXAnchor()
                .constraintEqualToAnchor(&cell.centerXAnchor()),
        ),
        Align::End => constraints.push(trailing()),
        Align::Fill => constraints.extend([leading(), trailing()]),
    }
    NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&constraints));

    let view: &NSView = cell.as_ref();
    view.retain()
}

struct TableInner {
    /// The view visible from outside. The table is one widget together with this
    /// scroll view.
    scroll: Retained<NSScrollView>,
    table: Retained<TableView>,
    columns: Rc<RefCell<Vec<TableColumn>>>,
    rows: Rc<RowsState>,
    mode: Cell<SelectionMode>,
    handler: SelectionHandler,
    sort_handler: SortHandler,
    silent: Rc<Cell<bool>>,
    /// The target that receives row clicks. `NSTableView` does not retain its target, so
    /// it is kept alive here.
    _action: Retained<ActionTarget>,
    /// Delegates and data sources are weak references, so they are held.
    _source: Retained<TableSource>,
}

/// A table with column headings (NSTableView).
///
/// Inside is an `NSTableView` in an `NSScrollView`, and [`Widget::native_view`]
/// returns that scroll view. **A scroll view has no height that fits its
/// contents**, so set the height with `set_sizing`.
#[derive(Clone)]
pub struct Table(Rc<TableInner>);

impl Widget for Table {
    fn native_view(&self) -> Retained<NSView> {
        let view: &NSView = self.0.scroll.as_ref();
        view.retain()
    }
    fn boxed_clone(&self) -> Box<dyn Widget> {
        Box::new(self.clone())
    }
}

crate::widgets::impl_sizing!(Table);

impl Table {
    pub(crate) fn new(mtm: MainThreadMarker) -> Self {
        let columns: Rc<RefCell<Vec<TableColumn>>> = Rc::new(RefCell::new(Vec::new()));
        let table = TableView::new(
            mtm,
            TableViewState {
                columns: columns.clone(),
            },
        );
        // 見出しが端まで伸びる、表向きの見た目。
        table.setStyle(NSTableViewStyle::FullWidth);
        table.setHeaderView(Some(&NSTableHeaderView::new(mtm)));
        // 行の区切りだけを引く。縦線まで引くと、macOS の表としては強すぎる。
        table.setGridStyleMask(NSTableViewGridLineStyle::SolidHorizontalGridLineMask);
        table.setAllowsMultipleSelection(false);
        // 単一選択でも「何も選ばれていない」状態を持てるようにする。
        table.setAllowsEmptySelection(true);
        // 列の順番は `set_columns` に渡した並びと同じ意味を持つので、
        // ユーザーに入れ替えさせない。幅の調整は自由にできる。
        table.setAllowsColumnReordering(false);
        table.setAllowsColumnResizing(true);
        table.setColumnAutoresizingStyle(
            NSTableViewColumnAutoresizingStyle::UniformColumnAutoresizingStyle,
        );
        table.setRowHeight(default_row_height(mtm));

        let rows: Rc<RowsState> = Rc::new(RowsState::default());
        let handler = SelectionHandler::default();
        let sort_handler = SortHandler::default();
        let silent = Rc::new(Cell::new(false));
        let source = TableSource::new(
            mtm,
            SourceState {
                columns: columns.clone(),
                rows: rows.clone(),
                handler: handler.clone(),
                sort_handler: sort_handler.clone(),
                silent: silent.clone(),
            },
        );
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(&*source)));
            table.setDelegate(Some(ProtocolObject::from_ref(&*source)));
        }

        // 行のクリックは `NSTableView` の action で受ける。表示専用のラベルや
        // アイコンを押すと、AppKit のヒットテストはセルではなくテーブルを
        // 返すため、セル側の `mouseDown` では届かない。ボタンや入力欄は
        // 自分がヒットするので、この action は呼ばれず二重に発火しない
        // (`List` と同じ切り分け)。
        let action = ActionTarget::new(mtm, {
            let rows = rows.clone();
            let table = table.clone();
            move || {
                let Ok(index) = usize::try_from(table.clickedRow()) else {
                    return;
                };
                if let Some(activation) = rows.activation(index) {
                    activation.emit();
                }
            }
        });
        unsafe {
            table.setTarget(Some(&action));
            table.setAction(Some(sel!(invoke:)));
        }

        let scroll = NSScrollView::new(mtm);
        scroll.setHasVerticalScroller(true);
        scroll.setDocumentView(Some(&table));

        Self(Rc::new(TableInner {
            scroll,
            table,
            columns,
            rows,
            mode: Cell::new(SelectionMode::Single),
            handler,
            sort_handler,
            silent,
            _action: action,
            _source: source,
        }))
    }

    /// Rebuilds the columns. Rows and selection stay; only the cells are rearranged.
    pub fn set_columns(&self, columns: &[TableColumn]) {
        // 列を入れ替えると AppKit は選択も並べ替えの指定も落とすので、
        // どちらも覚えて書き戻す。
        let picked = self.selection();
        let sorted = self.sort();
        *self.0.columns.borrow_mut() = columns.to_vec();
        // `tableColumns` はテーブルが持つ配列そのものなので、
        // たどりながら消すと AppKit に「列挙中の変更」として弾かれる。
        for column in self.0.table.tableColumns().to_vec() {
            self.0.table.removeTableColumn(&column);
        }
        let mtm = MainThreadMarker::from(&*self.0.table);
        for (index, spec) in columns.iter().enumerate() {
            let column = NSTableColumn::initWithIdentifier(
                NSTableColumn::alloc(mtm),
                &NSString::from_str(&format!("{COLUMN_ID_PREFIX}{index}")),
            );
            column.setTitle(&NSString::from_str(&spec.title));
            // 見出しを押したときの並べ替えは AppKit に任せる。
            // これを持つ列だけがヘッダーで押せるようになり、
            // 指標 (▲▼) の描き替えと向きの反転も AppKit が行う。
            if spec.sortable {
                let key = NSString::from_str(&format!("{COLUMN_ID_PREFIX}{index}"));
                let prototype = NSSortDescriptor::sortDescriptorWithKey_ascending(Some(&key), true);
                column.setSortDescriptorPrototype(Some(&prototype));
            }
            // 見出しの揃えは、その列のセルに合わせる。
            column.headerCell().setAlignment(text_alignment(spec.align));
            match spec.width {
                // 幅の指定がある列は、自動調整の対象から外して固定する。
                Some(width) => {
                    column.setWidth(width);
                    column.setMinWidth(width);
                    column.setMaxWidth(width);
                    column.setResizingMask(NSTableColumnResizingOptions::UserResizingMask);
                }
                // 指定が無い列だけで、余った幅を分け合う。
                None => {
                    column.setMinWidth(MIN_COLUMN_WIDTH);
                    column.setResizingMask(NSTableColumnResizingOptions::AutoresizingMask);
                }
            }
            self.0.table.addTableColumn(&column);
        }
        self.without_notifying(|this| {
            this.0.table.reloadData();
            this.apply_selection(&picked);
            // 列を作り直すと指定も落ちるので、並べ替えの指標を戻す。
            this.apply_sort(sorted);
        });
        // 大きさが決まっていれば、この場で余りを配る。まだなら
        // `setFrameSize:` が決まった時点で配る。
        distribute_widths(&self.0.table, &self.0.columns.borrow());
    }

    /// The number of columns.
    pub fn column_count(&self) -> usize {
        self.0.columns.borrow().len()
    }

    /// Rebuilds the rows. Indices change meaning, so the selection is cleared.
    ///
    /// However many rows there are, cells are built only for those on screen
    /// (`NSTableView` asks only for the rows it needs), so it is fine to pass all rows
    /// as is.
    pub fn set_rows(&self, rows: &[TableRow]) {
        self.0.rows.set_rows(rows);
        self.reload();
    }

    /// Passes rows whose cells can hold widgets, in a form that **builds each row when
    /// it becomes visible**.
    ///
    /// `count` is the number of rows, and `build` receives an index from 0 to
    /// `count - 1` and returns that row's contents ([`TableCells`]). It is called only
    /// for the rows on screen (and a few before and after), so opening stays fast even
    /// with hundreds of thousands of rows.
    ///
    /// The widgets in a row may be recreated every time it is built. Keep per-row state
    /// in the app's data and build from it inside `build`.
    ///
    /// ```no_run
    /// # use naui_macos::{Table, TableCells};
    /// # fn fill(table: &Table, cities: Vec<String>) {
    /// table.set_row_builder(cities.len(), move |index| {
    ///     Ok(TableCells::new().text(&cities[index]).text("13,960,000"))
    /// });
    /// # }
    /// ```
    pub fn set_row_builder(
        &self,
        count: usize,
        build: impl FnMut(usize) -> Result<TableCells> + 'static,
    ) {
        self.0.rows.set_builder(count, build);
        self.reload();
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
    /// # use naui_macos::{Table, TableCells};
    /// # fn fill(table: &Table, done: std::rc::Rc<Vec<bool>>) {
    /// let rows = done.clone();
    /// table.set_row_selectable(move |index| !rows[index]);
    /// # }
    /// ```
    pub fn set_row_selectable(&self, selectable: impl Fn(usize) -> bool + 'static) {
        self.0.rows.set_selectable(selectable);
    }

    /// Rebuilds the visible rows. The row count and selection stay.
    ///
    /// Use it when what the function passed to [`Table::set_row_builder`] returns has
    /// changed.
    pub fn refresh(&self) {
        let picked = self.selection();
        self.0.rows.forget_built();
        self.without_notifying(|this| {
            this.0.table.reloadData();
            this.apply_selection(&picked);
        });
    }

    /// Sets the row height in logical pixels. Passing 0 or less uses the default (a
    /// height that fits the text).
    ///
    /// Every row gets the same height. Increase it when buttons or images in cells do
    /// not fit the default.
    pub fn set_row_height(&self, height: f64) {
        let mtm = MainThreadMarker::from(&*self.0.table);
        let height = match height > 0.0 {
            true => height,
            false => default_row_height(mtm),
        };
        self.0.table.setRowHeight(height);
    }

    /// The row height (logical pixels).
    pub fn row_height(&self) -> f64 {
        self.0.table.rowHeight()
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
        self.0.table.setAllowsMultipleSelection(mode.is_multiple());
        self.without_notifying(|this| unsafe { this.0.table.deselectAll(None) });
    }

    pub fn selection_mode(&self) -> SelectionMode {
        self.0.mode.get()
    }

    /// The topmost of the selected rows.
    pub fn selected(&self) -> Option<usize> {
        self.selection().first().copied()
    }

    /// The selected rows (ascending). Zero or one for single selection.
    pub fn selection(&self) -> Vec<usize> {
        selected_indices(&self.0.table, self.len())
    }

    /// Scrolls until row `index` is visible. The selection does not change.
    ///
    /// If it is already visible nothing moves; otherwise it is aligned to the nearer
    /// edge (the top if it is above, the bottom if it is below). Does nothing if out of
    /// range.
    pub fn scroll_to_row(&self, index: usize) {
        if index < self.len() {
            self.0.table.scrollRowToVisible(index as isize);
        }
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
        self.without_notifying(|this| this.apply_selection(&picked));
    }

    /// Clears the whole selection without notifying.
    pub fn clear_selection(&self) {
        self.without_notifying(|this| unsafe { this.0.table.deselectAll(None) });
    }

    /// Selects one row through the same path as a user's selection (notifies).
    pub fn select(&self, index: usize) {
        self.select_many(&[index]);
    }

    /// Replaces the selection through the same path as a user's selection (notifies).
    pub fn select_many(&self, indices: &[usize]) {
        let picked = self.normalize(indices);
        // AppKit は同じ選択を選び直すとデリゲートを呼ばない。
        // 通知の回数をそろえるため、ここで 1 回だけ出す。
        self.without_notifying(|this| this.apply_selection(&picked));
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
        sort_from(&self.0.table)
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
        self.without_notifying(|this| this.apply_sort(sort));
    }

    /// The inner `NSTableView`. Public as a backend-specific escape hatch.
    ///
    /// Settings not in the common API, such as sorting or column appearance, are done
    /// from here.
    pub fn native_table(&self) -> Retained<NSTableView> {
        let table: &NSTableView = &self.0.table;
        table.retain()
    }

    /// Normalizes the requested selection into a form meaningful for this table.
    fn normalize(&self, indices: &[usize]) -> Vec<usize> {
        let rows = self.0.rows.clone();
        self.0
            .mode
            .get()
            .normalize_by(indices, |i| rows.is_selectable(i))
    }

    /// Rebuilds the rows. The selection is cleared.
    fn reload(&self) {
        self.without_notifying(|this| {
            this.0.table.reloadData();
            unsafe { this.0.table.deselectAll(None) };
        });
    }

    /// Applies the sort setting to the native side.
    fn apply_sort(&self, sort: Option<(usize, SortOrder)>) {
        let descriptors = match sort {
            None => NSArray::new(),
            Some((column, order)) => {
                let key = NSString::from_str(&format!("{COLUMN_ID_PREFIX}{column}"));
                let descriptor = NSSortDescriptor::sortDescriptorWithKey_ascending(
                    Some(&key),
                    order.is_ascending(),
                );
                NSArray::from_retained_slice(&[descriptor])
            }
        };
        self.0.table.setSortDescriptors(&descriptors);
    }

    fn apply_selection(&self, indices: &[usize]) {
        if indices.is_empty() {
            unsafe { self.0.table.deselectAll(None) };
            return;
        }
        let set = NSMutableIndexSet::new();
        for &index in indices {
            set.addIndex(index);
        }
        let set: &NSIndexSet = set.as_ref();
        self.0
            .table
            .selectRowIndexes_byExtendingSelection(set, false);
    }

    /// Operates with notifications from AppKit stopped.
    fn without_notifying(&self, f: impl FnOnce(&Self)) {
        let previous = self.0.silent.replace(true);
        f(self);
        self.0.silent.set(previous);
    }
}
