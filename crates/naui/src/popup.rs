use crate::sys;
use crate::Widget;
use naui_core::PopupItem;

/// A popup (context) menu shown on right-click.
///
/// It is not a widget laid out on screen, so it is not a [`Widget`]. Items are
/// identified by **a path**: the index at each level of submenus, separators
/// included (`[2]` is the third item, `[3, 0]` the first item in the submenu that is
/// the fourth item).
#[derive(Clone)]
pub struct PopupMenu(pub(crate) sys::PopupMenu);

impl_backend!(PopupMenu);

impl PopupMenu {
    /// Rebuilds the items. The previous items (and their check states) are removed.
    #[inline]
    pub fn set_items(&self, items: &[PopupItem]) {
        self.0.set_items(items)
    }

    /// The number of top-level items, separators included.
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Makes this menu appear on right-click on the widget. It can be attached to any
    /// number of widgets.
    #[inline]
    pub fn attach(&self, widget: &dyn Widget) {
        self.0.attach(widget.as_backend())
    }

    /// Shows it from code. The position is in logical pixels from the top left of
    /// `widget`.
    #[inline]
    pub fn open_at(&self, widget: &dyn Widget, x: f64, y: f64) {
        self.0.open_at(widget.as_backend(), x, y)
    }

    #[inline]
    pub fn close(&self) {
        self.0.close()
    }

    /// Turns the check mark of the item at `path` on or off, without notifying.
    ///
    /// Does nothing for items not made with
    /// [`PopupItem::checked`](crate::PopupItem::checked).
    #[inline]
    pub fn set_item_checked(&self, path: &[usize], checked: bool) {
        self.0.set_item_checked(path, checked)
    }

    /// Whether the check mark of the item at `path` is on. `false` for items without a
    /// check mark and out of range.
    ///
    /// Choosing a check item flips it before [`on_select`](Self::on_select) is called.
    #[inline]
    pub fn is_item_checked(&self, path: &[usize]) -> bool {
        self.0.is_item_checked(path)
    }

    /// Notifies through the same path as a user's choice (a check item flips).
    ///
    /// Separators, submenu headings and items that cannot be chosen are ignored.
    #[inline]
    pub fn select(&self, path: &[usize]) {
        self.0.select(path)
    }

    /// Called with the item's path when an item is chosen.
    #[inline]
    pub fn on_select(&self, f: impl FnMut(&[usize]) + 'static) {
        self.0.on_select(f)
    }
}
