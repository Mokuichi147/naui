use crate::sys;
use naui_core::MenuSpec;

/// The OS's application menu, attached to the top of the window.
///
/// Not a [`Widget`](crate::Widget). Attach it with
/// [`Window::set_menu_bar`](crate::Window::set_menu_bar). Every time an item is
/// clicked, [`on_activate`](Self::on_activate) is called with its **(heading index,
/// item index)**. The item index is the position in the sequence including
/// separators; separators are never returned.
#[derive(Clone)]
pub struct MenuBar(pub(crate) sys::MenuBar);

impl_backend!(MenuBar);

impl MenuBar {
    /// Rebuilds the menus. The previous menus are removed.
    ///
    /// The heading index is the position in the sequence passed, and the item index is
    /// the position in the sequence including separators.
    #[inline]
    pub fn set_menus(&self, menus: &[MenuSpec]) {
        self.0.set_menus(menus)
    }

    /// The number of headings.
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The number of items under one heading, separators included. `0` if out of
    /// range.
    #[inline]
    pub fn menu_len(&self, menu: usize) -> usize {
        self.0.menu_len(menu)
    }

    /// Enables or disables one item. Does nothing for separators or out of range.
    #[inline]
    pub fn set_item_enabled(&self, menu: usize, item: usize, enabled: bool) {
        self.0.set_item_enabled(menu, item, enabled)
    }

    /// Whether the item can be clicked now. `false` for separators and out of range.
    #[inline]
    pub fn is_item_enabled(&self, menu: usize, item: usize) -> bool {
        self.0.is_item_enabled(menu, item)
    }

    /// Enables or disables the whole menu bar. Per-item settings are kept.
    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// Runs the item as if the user had chosen it.
    ///
    /// Does nothing for separators, disabled items or out of range.
    #[inline]
    pub fn activate(&self, menu: usize, item: usize) {
        self.0.activate(menu, item)
    }

    /// Called with the heading and item indices when an item is clicked. Setting it
    /// again removes the previous callback.
    #[inline]
    pub fn on_activate(&self, f: impl FnMut(usize, usize) + 'static) {
        self.0.on_activate(f)
    }
}
