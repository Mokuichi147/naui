use crate::sys;
use naui_core::ToolbarItem;

/// A row of frequently used actions attached to the top of the window.
///
/// Not a [`Widget`](crate::Widget). Attach it with
/// [`Window::set_toolbar`](crate::Window::set_toolbar). Unlike navigation, it
/// **has no selected item**; every click calls [`on_activate`](Self::on_activate)
/// with the index. The index is the position in the sequence including separators;
/// separators are never returned.
#[derive(Clone)]
pub struct Toolbar(pub(crate) sys::Toolbar);

impl_backend!(Toolbar);

impl Toolbar {
    /// Rebuilds the items. The previous items are removed.
    ///
    /// The index is the position in the sequence including separators.
    #[inline]
    pub fn set_items(&self, items: &[ToolbarItem]) {
        self.0.set_items(items)
    }

    /// The number of items, separators included.
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Enables or disables one item. Does nothing for separators or out of range.
    #[inline]
    pub fn set_item_enabled(&self, index: usize, enabled: bool) {
        self.0.set_item_enabled(index, enabled)
    }

    /// Whether the item can be clicked now. `false` for separators and out of range.
    #[inline]
    pub fn is_item_enabled(&self, index: usize) -> bool {
        self.0.is_item_enabled(index)
    }

    /// Enables or disables the whole toolbar. Per-item settings are kept.
    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// Runs the item as if the user had clicked it.
    ///
    /// Does nothing for separators, disabled items or out of range.
    #[inline]
    pub fn activate(&self, index: usize) {
        self.0.activate(index)
    }

    /// Called with the index when an item is clicked. Setting it again removes the
    /// previous callback.
    #[inline]
    pub fn on_activate(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_activate(f)
    }
}
