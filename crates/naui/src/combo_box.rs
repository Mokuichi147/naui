use crate::sys;

/// A drop-down for choosing one item.
#[derive(Clone)]
pub struct ComboBox(pub(crate) sys::ComboBox);

impl_widget!(ComboBox);

impl ComboBox {
    /// Rebuilds the items. Indices change meaning, so the selection is cleared. No
    /// notification is sent.
    #[inline]
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        self.0.set_items(items)
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
    pub fn selected(&self) -> Option<usize> {
        self.0.selected()
    }

    /// Changes the selection without notifying, if the index is in range.
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// Clears the selection without notifying.
    #[inline]
    pub fn clear_selection(&self) {
        self.0.clear_selection()
    }

    /// Selects an item through the same path as a user's choice and notifies once.
    #[inline]
    pub fn select(&self, index: usize) {
        self.0.select(index)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// Called with the index of the chosen item when the selection changes.
    #[inline]
    pub fn on_select(&self, f: impl FnMut(usize) + 'static) {
        self.0.on_select(f)
    }
}
