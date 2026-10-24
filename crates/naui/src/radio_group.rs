use crate::sys;
use naui_core::Orientation;

/// A radio group that lays out options and lets the user choose exactly one.
///
/// Right after creation and right after [`set_items`](Self::set_items), nothing is
/// selected.
#[derive(Clone)]
pub struct RadioGroup(pub(crate) sys::RadioGroup);

impl_widget!(RadioGroup);

impl RadioGroup {
    /// Rebuilds the options and clears the selection. No selection notification is
    /// sent.
    #[inline]
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        self.0.set_items(items)
    }

    /// The number of options.
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The currently selected option. `None` if nothing is selected.
    #[inline]
    pub fn selected(&self) -> Option<usize> {
        self.0.selected()
    }

    /// Selects an in-range option without notifying. Does nothing if out of range.
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// Clears the selection without notifying.
    #[inline]
    pub fn clear_selection(&self) {
        self.0.clear_selection()
    }

    /// Selects an in-range option and notifies, as if the user had chosen it.
    #[inline]
    pub fn select(&self, index: usize) {
        self.0.select(index)
    }

    /// The direction the options are laid out in. Vertical by default.
    #[inline]
    pub fn set_orientation(&self, orientation: Orientation) {
        self.0.set_orientation(orientation)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// Called with the index when an option is chosen. Setting it again removes the
    /// previous callback.
    #[inline]
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select(f)
    }
}
