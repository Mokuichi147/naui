use crate::sys;
use crate::Widget;
use naui_core::PopupItem;

/// A popup (context) menu shown on right-click.
///
/// It is not a widget laid out on screen, so it is not a [`Widget`].
#[derive(Clone)]
pub struct PopupMenu(pub(crate) sys::PopupMenu);

impl_backend!(PopupMenu);

impl PopupMenu {
    /// Rebuilds the items. Indices count **the sequence including separators**.
    #[inline]
    pub fn set_items(&self, items: &[PopupItem]) {
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

    /// Notifies through the same path as a user's choice.
    ///
    /// Separators and items that cannot be chosen are ignored.
    #[inline]
    pub fn select(&self, index: usize) {
        self.0.select(index)
    }

    /// Called with the index when an item is chosen.
    #[inline]
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select(f)
    }
}
