use crate::sys;
use crate::Widget;
use naui_core::{SelectionMode, TreeItem};

/// A list whose nested items can be expanded and collapsed. It scrolls by itself.
///
/// Items are referred to by the sequence of child indices from the root (a path).
/// `[0, 2]` is "the third child of the first root", and an empty path means "no
/// selection".
///
/// Its height is not determined by its contents, so set it with
/// [`Tree::set_sizing`].
///
/// With [`SelectionMode::Multiple`], several items can be selected with ⌘ / Ctrl or
/// Shift, as in a [`List`](crate::List). Each item is selected on its own: selecting
/// a branch does not select its children. Selected items inside a branch that is
/// collapsed stay selected while hidden. Adding or removing items with ⌘ / Ctrl keeps
/// them; a plain click or a Shift range selects anew and drops them, as for rows off
/// screen in a long `List`.
///
/// Items hold either strings ([`Tree::set_items`]) or widgets ([`Tree::set_rows`]).
#[derive(Clone)]
pub struct Tree(pub(crate) sys::Tree);

impl_widget!(Tree);

impl Tree {
    /// Rebuilds the items. Paths change meaning, so the selection is cleared.
    ///
    /// Expansion returns to what [`TreeItem::expanded`] says.
    #[inline]
    pub fn set_items(&self, items: &[TreeItem]) {
        self.0.set_items(items)
    }

    /// Rebuilds the items with widgets as their content. Paths change meaning, so the
    /// selection is cleared.
    ///
    /// Expansion returns to what [`TreeRow::expanded`] says. Pass widgets that are in
    /// no other container, as for [`List::set_rows`](crate::List::set_rows) (the
    /// guide's section on moving widgets explains why). A widget already in this tree
    /// can be passed again. If the app moves a widget elsewhere afterwards, the tree
    /// leaves it alone and its row stays empty.
    ///
    /// Controls inside a row receive their own clicks: pressing one neither selects
    /// the item nor expands or collapses it. Every row is built when it is
    /// passed, including those inside collapsed branches.
    ///
    /// ```no_run
    /// # use naui::{Tree, TreeRow, Ui};
    /// # fn fill(ui: &Ui, tree: &Tree) -> naui::Result<()> {
    /// let src = TreeRow::new(&ui.label("src")?)
    ///     .expanded(true)
    ///     .child(TreeRow::new(&ui.checkbox("main.rs")?));
    /// tree.set_rows(&[src]);
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub fn set_rows(&self, rows: &[TreeRow]) {
        self.0
            .set_rows(&rows.iter().map(|row| row.0.clone()).collect::<Vec<_>>())
    }

    /// The total number of items, descendants included.
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Switches between single and multiple selection. Single selection by default.
    ///
    /// Switching clears the selection without notifying, as [`List`](crate::List)
    /// does.
    #[inline]
    pub fn set_selection_mode(&self, mode: SelectionMode) {
        self.0.set_selection_mode(mode)
    }

    #[inline]
    pub fn selection_mode(&self) -> SelectionMode {
        self.0.selection_mode()
    }

    /// The path of the first selected item, in the order the tree shows them. `None`
    /// if nothing is selected.
    #[inline]
    pub fn selected(&self) -> Option<Vec<usize>> {
        self.0.selected()
    }

    /// The paths of the selected items in the order the tree shows them (parents
    /// first), including items hidden inside collapsed branches.
    #[inline]
    pub fn selection(&self) -> Vec<Vec<usize>> {
        self.0.selection()
    }

    /// Scrolls until the item at `path` is visible. The selection does not change.
    ///
    /// If it is inside a collapsed branch, its ancestors are expanded first (as with
    /// [`set_selected`](Self::set_selected), expansion is not notified). If it is
    /// already visible nothing moves. Does nothing for a path that does not exist.
    #[inline]
    pub fn scroll_to_item(&self, path: &[usize]) {
        self.0.scroll_to_item(path)
    }

    /// Selects one item without notifying.
    ///
    /// Passing an unselectable item or a path that does not exist clears the
    /// selection. Items inside a collapsed branch are selected after their ancestors
    /// are expanded so they become visible.
    #[inline]
    pub fn set_selected(&self, path: &[usize]) {
        self.0.set_selected(path)
    }

    /// Replaces the selection without notifying.
    ///
    /// Paths that point at no item and items that cannot be selected are left out
    /// (with single selection, only the first remaining item is selected). Branches
    /// holding the selected items are expanded so they become visible.
    #[inline]
    pub fn set_selection(&self, paths: &[Vec<usize>]) {
        self.0.set_selection(paths)
    }

    /// Clears the selection without notifying.
    #[inline]
    pub fn clear_selection(&self) {
        self.0.clear_selection()
    }

    /// Selects one item through the same path as a user's selection (notifies).
    #[inline]
    pub fn select(&self, path: &[usize]) {
        self.0.select(path)
    }

    /// Replaces the selection through the same path as a user's selection
    /// (notifies). Paths are left out as in [`Tree::set_selection`].
    #[inline]
    pub fn select_many(&self, paths: &[Vec<usize>]) {
        self.0.select_many(paths)
    }

    /// Called with the path of the first selected item ([`Tree::selected`]) every
    /// time the selection changes.
    ///
    /// When the selection is cleared, it is called with an empty path. With
    /// multiple selection, read [`Tree::selection`] inside it for every selected
    /// item.
    #[inline]
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.on_select(f)
    }

    /// Whether the item is expanded.
    ///
    /// Even an item hidden inside a collapsed branch returns `true` if it is
    /// remembered as expanded (reopening the parent shows it expanded as before).
    #[inline]
    pub fn is_expanded(&self, path: &[usize]) -> bool {
        self.0.is_expanded(path)
    }

    /// Changes expansion without notifying. Expanding also expands the ancestors.
    #[inline]
    pub fn set_expanded(&self, path: &[usize], expanded: bool) {
        self.0.set_expanded(path, expanded)
    }

    /// Expands through the same path as a user's action (notifies).
    #[inline]
    pub fn expand(&self, path: &[usize]) {
        self.0.expand(path)
    }

    /// Collapses through the same path as a user's action (notifies).
    #[inline]
    pub fn collapse(&self, path: &[usize]) {
        self.0.collapse(path)
    }

    /// Expands every branch without notifying.
    #[inline]
    pub fn expand_all(&self) {
        self.0.expand_all()
    }

    /// Collapses every branch without notifying.
    #[inline]
    pub fn collapse_all(&self) {
        self.0.collapse_all()
    }

    /// Called with the item's path and its new state when its expansion changes.
    #[inline]
    pub fn on_expand(&self, f: impl FnMut(&[usize], bool) + 'static) {
        self.0.on_expand(f)
    }
}

/// One item to put in a tree, with a widget as its content.
///
/// [`TreeItem`] is a convenience API for trees that need only strings. Items that
/// combine icons, several labels, check boxes, buttons and so on are built with
/// `Grid` / `Stack` and wrapped in a `TreeRow`, as rows of a [`List`](crate::List)
/// are wrapped in a [`ListRow`](crate::ListRow).
///
/// ```no_run
/// # use naui::{TreeRow, Widget};
/// # fn row(folder: &dyn Widget, file: &dyn Widget) {
/// let row = TreeRow::new(folder)
///     .expanded(true)
///     .child(TreeRow::new(file).selectable(false));
/// # let _ = row;
/// # }
/// ```
#[derive(Clone)]
pub struct TreeRow(pub(crate) sys::TreeRow);

impl_backend!(TreeRow);

impl TreeRow {
    /// Uses a widget as the content of one item. By default the item can be selected,
    /// starts collapsed and has no children.
    #[inline]
    pub fn new(content: &dyn Widget) -> Self {
        Self(sys::TreeRow::new(content.as_backend()))
    }

    /// Set to `false` for items where only the controls inside are operated.
    ///
    /// As with [`TreeItem::enabled`], its descendants cannot be selected either
    /// ([`TreeItem::selectable`]), and the branch can still be expanded and collapsed.
    /// Unlike `TreeItem::enabled(false)`, the row is not dimmed and the controls inside
    /// keep working.
    #[inline]
    pub fn selectable(self, selectable: bool) -> Self {
        Self(self.0.selectable(selectable))
    }

    #[inline]
    pub fn is_selectable(&self) -> bool {
        self.0.is_selectable()
    }

    /// Sets whether it is shown expanded from the start (collapsed by default).
    ///
    /// This takes effect only when [`Tree::set_rows`] is called; after that, the widget
    /// owns the expansion state.
    #[inline]
    pub fn expanded(self, expanded: bool) -> Self {
        Self(self.0.expanded(expanded))
    }

    #[inline]
    pub fn is_expanded(&self) -> bool {
        self.0.is_expanded()
    }

    /// Replaces the child items.
    #[inline]
    pub fn children(self, children: impl IntoIterator<Item = TreeRow>) -> Self {
        Self(self.0.children(children.into_iter().map(|row| row.0)))
    }

    /// Adds one child item.
    #[inline]
    pub fn child(self, child: TreeRow) -> Self {
        Self(self.0.child(child.0))
    }

    /// Whether the item has no children (cannot be expanded).
    #[inline]
    pub fn is_leaf(&self) -> bool {
        self.0.is_leaf()
    }
}
