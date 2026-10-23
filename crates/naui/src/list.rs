use crate::sys;
use crate::Widget;
use naui_core::{ListItem, Point, SelectionMode};

/// A list of selectable rows. It scrolls by itself.
///
/// Without a height set, it follows the height of all its rows; with a fixed height
/// or `Fill`, it scrolls whatever overflows.
#[derive(Clone)]
pub struct List(pub(crate) sys::List);

impl_widget!(List);

impl List {
    /// Rebuilds the rows. Indices change meaning, so the selection is cleared.
    #[inline]
    pub fn set_items(&self, items: &[ListItem]) {
        self.0.set_items(items)
    }

    /// Rebuilds the rows. Both plain rows and rows with custom content are drawn
    /// through this path.
    ///
    /// Controls inside a row can have their own callbacks as usual.
    #[inline]
    pub fn set_rows(&self, rows: &[ListRow]) {
        self.0
            .set_rows(&rows.iter().map(|row| row.0.clone()).collect::<Vec<_>>())
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    pub fn set_selection_mode(&self, mode: SelectionMode) {
        self.0.set_selection_mode(mode)
    }

    #[inline]
    pub fn selection_mode(&self) -> SelectionMode {
        self.0.selection_mode()
    }

    /// The first selected row.
    #[inline]
    pub fn selected(&self) -> Option<usize> {
        self.0.selected()
    }

    /// Returns the selected rows in ascending order.
    #[inline]
    pub fn selection(&self) -> Vec<usize> {
        self.0.selection()
    }

    /// Scrolls until row `index` is visible. The selection does not change.
    ///
    /// If it is already visible nothing moves; otherwise it is aligned to the nearer
    /// edge (the top if it is above, the bottom if it is below). Does nothing if out of
    /// range.
    #[inline]
    pub fn scroll_to_row(&self, index: usize) {
        self.0.scroll_to_row(index)
    }

    /// Selects a single row without notifying.
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// Replaces the selection without notifying.
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

    /// Called with the row's index when a row is activated (double-clicked, or Enter
    /// while the row has focus).
    ///
    /// To open everything in a multiple selection, read [`List::selection`] inside it.
    /// Not called for rows of `ListItem`s that cannot be selected (`enabled(false)`).
    #[inline]
    pub fn on_row_activate(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_row_activate(f)
    }

    /// Called with the row's index and the click position when a row is right-clicked.
    ///
    /// The position is in this list's coordinates (logical pixels, y pointing down) and
    /// can be passed straight to [`crate::PopupMenu::open_at`]. The selection does not
    /// change. Not called for empty space without rows.
    #[inline]
    pub fn on_context_menu(&self, f: impl FnMut(usize, Point) + 'static) {
        self.0.on_context_menu(f)
    }
}

/// One row to put in a list.
///
/// [`ListItem`] is a convenience API for lists that need only strings. Rows that
/// combine icons, several labels, check boxes, trailing buttons and so on, as in a
/// settings screen, are built with `Grid` / `Stack` and wrapped in a `ListRow`.
///
/// ```no_run
/// # use naui::{ListRow, Widget};
/// # fn row(content: &dyn Widget) {
/// let row = ListRow::new(content).selectable(false);
/// row.on_activate(|| println!("row clicked"));
/// # let _ = row;
/// # }
/// ```
#[derive(Clone)]
pub struct ListRow(pub(crate) sys::ListRow);

impl_backend!(ListRow);

impl ListRow {
    /// Uses a widget as the content of one row. By default the whole row can be
    /// selected too.
    #[inline]
    pub fn new(content: &dyn Widget) -> Self {
        Self(sys::ListRow::new(content.as_backend()))
    }

    /// Set to `false` for rows where only the controls inside are operated.
    #[inline]
    pub fn selectable(self, selectable: bool) -> Self {
        Self(self.0.selectable(selectable))
    }

    #[inline]
    pub fn is_selectable(&self) -> bool {
        self.0.is_selectable()
    }

    /// What to do when a label, icon or empty space inside the row is clicked.
    ///
    /// When a control such as a check box, button or text field is clicked directly, it
    /// is not called (each control receives its own clicks), so the same action does
    /// not fire twice.
    ///
    /// Whether a `GtkListBoxRow` is clickable is decided when the row is built, so set
    /// this before passing it to [`List::set_rows`].
    #[inline]
    pub fn on_activate(&self, f: impl FnMut() + 'static) {
        self.0.on_activate(f)
    }
}
