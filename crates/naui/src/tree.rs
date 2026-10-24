use crate::sys;
use naui_core::TreeItem;

/// A list whose nested items can be expanded and collapsed. It scrolls by itself.
///
/// Items are referred to by the sequence of child indices from the root (a path).
/// `[0, 2]` is "the third child of the first root", and an empty path means "no
/// selection".
///
/// Its height is not determined by its contents, so set it with
/// [`Tree::set_sizing`].
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

    /// The total number of items, descendants included.
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The path of the selected item. `None` if nothing is selected.
    #[inline]
    pub fn selected(&self) -> Option<Vec<usize>> {
        self.0.selected()
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

    /// Called with the path of the selected item when the selection changes.
    ///
    /// When the selection is cleared, it is called with an empty path.
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
