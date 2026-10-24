use crate::sys;
use crate::Widget;
use naui_core::{Result, SelectionMode, SortOrder, TableColumn, TableRow};

/// A table with column headings. It scrolls by itself.
///
/// Its height is not determined by its contents, so set it with
/// [`Table::set_sizing`].
#[derive(Clone)]
pub struct Table(pub(crate) sys::Table);

impl_widget!(Table);

impl Table {
    /// Rebuilds the columns. Rows and selection stay; only the cells are rearranged.
    ///
    /// The sort setting also stays if that column can still be sorted.
    #[inline]
    pub fn set_columns(&self, columns: &[TableColumn]) {
        self.0.set_columns(columns)
    }

    /// The number of columns.
    #[inline]
    pub fn column_count(&self) -> usize {
        self.0.column_count()
    }

    /// Rebuilds the rows. Indices change meaning, so the selection is cleared.
    ///
    /// With many rows, `GtkListBoxRow`s are created only for the rows on screen (the
    /// spacers above and below carry the height of the rest, so the scroll bar length
    /// still reflects all the rows).
    #[inline]
    pub fn set_rows(&self, rows: &[TableRow]) {
        self.0.set_rows(rows)
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
    /// # use naui::{Table, TableCells};
    /// # fn fill(table: &Table, cities: Vec<String>) {
    /// table.set_row_builder(cities.len(), move |index| {
    ///     Ok(TableCells::new().text(&cities[index]))
    /// });
    /// # }
    /// ```
    #[inline]
    pub fn set_row_builder(
        &self,
        count: usize,
        mut build: impl FnMut(usize) -> Result<TableCells> + 'static,
    ) {
        self.0
            .set_row_builder(count, move |index| build(index).map(|cells| cells.0))
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
    /// # use naui::{Table, TableCells};
    /// # fn fill(table: &Table, done: std::rc::Rc<Vec<bool>>) {
    /// let rows = done.clone();
    /// table.set_row_selectable(move |index| !rows[index]);
    /// # }
    /// ```
    #[inline]
    pub fn set_row_selectable(&self, selectable: impl Fn(usize) -> bool + 'static) {
        self.0.set_row_selectable(selectable)
    }

    /// Rebuilds the visible rows. The row count and selection stay.
    #[inline]
    pub fn refresh(&self) {
        self.0.refresh()
    }

    /// Sets the row height in logical pixels. Passing 0 or less measures it from a
    /// built row.
    ///
    /// Every row gets the same height. Rows off screen are filled in as "row count ×
    /// this height", so if row heights are not uniform the scroll bar length will be
    /// slightly off from reality.
    #[inline]
    pub fn set_row_height(&self, height: f64) {
        self.0.set_row_height(height)
    }

    /// The row height (logical pixels). 0 if no row has been built yet.
    #[inline]
    pub fn row_height(&self) -> f64 {
        self.0.row_height()
    }

    /// The number of rows.
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Changes how rows are selected. The meaning of the selection changes, so the
    /// selection is cleared.
    #[inline]
    pub fn set_selection_mode(&self, mode: SelectionMode) {
        self.0.set_selection_mode(mode)
    }

    #[inline]
    pub fn selection_mode(&self) -> SelectionMode {
        self.0.selection_mode()
    }

    /// The topmost of the selected rows.
    #[inline]
    pub fn selected(&self) -> Option<usize> {
        self.0.selected()
    }

    /// The selected rows (ascending). Zero or one for single selection.
    ///
    /// Rows outside the window are included. `GtkListBox` knows only the rows that are
    /// built, so the remembered selection is returned.
    #[inline]
    pub fn selection(&self) -> Vec<usize> {
        self.0.selection()
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
    #[inline]
    pub fn scroll_to_row(&self, index: usize) {
        self.0.scroll_to_row(index)
    }

    /// Selects exactly one row without notifying.
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// Replaces the selection without notifying.
    ///
    /// Out-of-range rows, unselectable rows and duplicates are removed, and with single
    /// selection only the first remains ([`SelectionMode::normalize_by`]).
    #[inline]
    pub fn set_selection(&self, indices: &[usize]) {
        self.0.set_selection(indices)
    }

    /// Clears the whole selection without notifying.
    #[inline]
    pub fn clear_selection(&self) {
        self.0.clear_selection()
    }

    /// Selects one row through the same path as a user's selection (notifies).
    #[inline]
    pub fn select(&self, index: usize) {
        self.0.select(index)
    }

    /// Replaces the selection through the same path as a user's selection (notifies).
    #[inline]
    pub fn select_many(&self, indices: &[usize]) {
        self.0.select_many(indices)
    }

    /// Called with the indices of the selected rows every time the selection changes.
    #[inline]
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.on_select(f)
    }

    /// Called with the clicked column and direction when a heading is clicked and the
    /// sort setting changes.
    ///
    /// Only columns with [`TableColumn::sortable`] set can be clicked. **Sorting the
    /// rows is the app's job**: when notified, pass the sorted rows again with
    /// [`Table::set_rows`] (`set_rows` clears the selection, so reselect if needed).
    #[inline]
    pub fn on_sort(&self, f: impl FnMut(usize, SortOrder) + 'static) {
        self.0.on_sort(f)
    }

    /// The current sort setting (column and direction). `None` if no heading has been
    /// clicked.
    #[inline]
    pub fn sort(&self) -> Option<(usize, SortOrder)> {
        self.0.sort()
    }

    /// Replaces the sort setting without notifying. Only the heading's indicator
    /// changes.
    ///
    /// Pointing at a column that cannot be sorted, or one out of range, clears the
    /// setting.
    #[inline]
    pub fn set_sort(&self, sort: Option<(usize, SortOrder)>) {
        self.0.set_sort(sort)
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
/// # use naui::{Table, TableCells};
/// # fn fill(table: &Table, cities: Vec<String>) {
/// table.set_row_builder(cities.len(), move |index| {
///     Ok(TableCells::new().text(&cities[index]).text("13,960,000"))
/// });
/// # }
/// ```
#[derive(Clone, Default)]
pub struct TableCells(pub(crate) sys::TableCells);

impl_backend!(TableCells);

impl TableCells {
    /// Creates a row with no cells.
    #[inline]
    pub fn new() -> Self {
        Self(sys::TableCells::new())
    }

    /// Adds one text cell. Alignment follows the column's setting.
    #[inline]
    pub fn text(self, text: impl Into<String>) -> Self {
        Self(self.0.text(text))
    }

    /// Adds one widget cell.
    #[inline]
    pub fn cell(self, content: &dyn Widget) -> Self {
        Self(self.0.cell(content.as_backend()))
    }

    /// Whether the whole row can be selected (it can by default).
    #[inline]
    pub fn selectable(self, selectable: bool) -> Self {
        Self(self.0.selectable(selectable))
    }

    #[inline]
    pub fn is_selectable(&self) -> bool {
        self.0.is_selectable()
    }

    /// The number of columns.
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// What to do when a cell or empty space in the row is clicked.
    ///
    /// When a button or text field is clicked directly, that control receives the
    /// click, so this is not called.
    #[inline]
    pub fn on_activate(&self, f: impl FnMut() + 'static) {
        self.0.on_activate(f)
    }
}
