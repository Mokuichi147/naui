use crate::sys;
use naui_core::{EventResponse, KeyEvent};

/// A field where the user can either choose from suggestions or type freely.
///
/// The value is a string, empty right after creation.
#[derive(Clone)]
pub struct EditableComboBox(pub(crate) sys::EditableComboBox);

impl_widget!(EditableComboBox);

impl EditableComboBox {
    /// Notified when a key is pressed. Returning `Handled` keeps the key from reaching
    /// the field and the window (so you can stop newlines or confirmation).
    ///
    /// Keys pressed while an IME is composing are not delivered.
    #[inline]
    pub fn on_key_down(&self, f: impl FnMut(&KeyEvent) -> EventResponse + 'static) {
        self.0.on_key_down(f)
    }

    /// Notified when focus enters (`true`) and leaves (`false`).
    ///
    /// Called only when focus moves within the window. Not called when the window
    /// itself becomes inactive.
    #[inline]
    pub fn on_focus_change(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_focus_change(f)
    }

    /// Rebuilds the suggestions. **The entered text does not change**, and no
    /// notification is sent.
    #[inline]
    pub fn set_items<S: AsRef<str>>(&self, items: &[S]) {
        self.0.set_items(items)
    }

    /// The number of suggestions.
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The entered text.
    #[inline]
    pub fn text(&self) -> String {
        self.0.text()
    }

    /// Replaces the text from code. `on_change` is not called.
    #[inline]
    pub fn set_text(&self, text: &str) {
        self.0.set_text(text)
    }

    /// The position of the suggestion that **exactly matches** the entered text.
    ///
    /// `None` if the typed text matches no suggestion.
    #[inline]
    pub fn selected(&self) -> Option<usize> {
        self.0.selected()
    }

    /// Chooses an in-range suggestion without notifying. Does nothing if out of range.
    #[inline]
    pub fn set_selected(&self, index: usize) {
        self.0.set_selected(index)
    }

    /// Empties the text without notifying.
    #[inline]
    pub fn clear(&self) {
        self.0.clear()
    }

    /// Chooses an in-range suggestion and notifies, as if the user had chosen it.
    #[inline]
    pub fn select(&self, index: usize) {
        self.0.select(index)
    }

    #[inline]
    pub fn set_placeholder(&self, text: &str) {
        self.0.set_placeholder(text)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// Called with the current contents every time the text changes. Called for both
    /// typing and choosing a suggestion. Setting it again removes the previous one.
    #[inline]
    pub fn on_change(&self, f: impl FnMut(&str) + 'static) {
        self.0.on_change(f)
    }
}
