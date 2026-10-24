use crate::sys;
use naui_core::{SidebarItem, SidebarSection};

/// A sidebar attached to the left of the window.
///
/// Attach it with [`Window::set_sidebar`](crate::Window::set_sidebar). It is not
/// placed in a layout, so it is not a [`Widget`](crate::Widget).
#[derive(Clone)]
pub struct Sidebar(pub(crate) sys::Sidebar);

impl_backend!(Sidebar);

impl Sidebar {
    /// Lays out items by group. Each call replaces them.
    ///
    /// The selected item stays selected (without notifying) if the same running number
    /// can still be selected.
    #[inline]
    pub fn set_sections(&self, sections: &[SidebarSection]) {
        self.0.set_sections(sections)
    }

    /// Lays out items as a single group without a heading.
    #[inline]
    pub fn set_items(&self, items: &[SidebarItem]) {
        self.0.set_items(items)
    }

    /// The number of items across groups.
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The running number of the selected item.
    #[inline]
    pub fn selected(&self) -> Option<usize> {
        self.0.selected()
    }

    /// Changes the selection without notifying. Out-of-range and unselectable items are
    /// ignored.
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// Clears the selection (without notifying).
    #[inline]
    pub fn clear_selection(&self) {
        self.0.clear_selection()
    }

    /// Selects as if the user had chosen it, and notifies.
    ///
    /// Out-of-range and unselectable items are ignored. Notifies even if the item is
    /// already selected.
    #[inline]
    pub fn select(&self, index: usize) {
        self.0.select(index)
    }

    /// The handler called when an item is chosen. The argument is the running number.
    #[inline]
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select(f)
    }

    /// The sidebar width (logical pixels). The default is
    /// [`DEFAULT_SIDEBAR_WIDTH`](crate::DEFAULT_SIDEBAR_WIDTH).
    ///
    /// The user can change the width by dragging the divider (the minimum is
    /// [`SIDEBAR_MIN_WIDTH`](crate::SIDEBAR_MIN_WIDTH)). The width after a change is
    /// returned by [`width`](Self::width) and delivered through
    /// [`on_resize`](Self::on_resize). This call does not call `on_resize`.
    #[inline]
    pub fn set_width(&self, width: f64) {
        self.0.set_width(width)
    }

    /// The sidebar width. Even while collapsed, returns the width it has when expanded.
    #[inline]
    pub fn width(&self) -> f64 {
        self.0.width()
    }

    /// Called with the new width every time the user changes it with the divider.
    #[inline]
    pub fn on_resize(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_resize(f)
    }

    /// Collapses (`true`) or expands (`false`) the sidebar.
    ///
    /// Items, selection and width are kept while collapsed.
    /// [`on_collapse`](Self::on_collapse) is not called.
    #[inline]
    pub fn set_collapsed(&self, collapsed: bool) {
        self.0.set_collapsed(collapsed)
    }

    /// The handler called when the user collapses or expands the sidebar. The argument
    /// is whether it is now collapsed.
    ///
    /// Called when the sidebar button is used; not called by
    /// [`set_collapsed`](Self::set_collapsed).
    #[inline]
    pub fn on_collapse(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_collapse(f)
    }

    /// Whether the sidebar is collapsed.
    ///
    /// This looks at the pane's own visibility setting (`get_visible`). `is_visible`
    /// reports whether it is visible including its parents, so it would be false before
    /// the window is shown.
    #[inline]
    pub fn is_collapsed(&self) -> bool {
        self.0.is_collapsed()
    }
}
