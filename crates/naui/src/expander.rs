use crate::sys;
use crate::Widget;

/// A container whose contents are shown and hidden by clicking its heading
/// (`GtkExpander`).
#[derive(Clone)]
pub struct Expander(pub(crate) sys::Expander);

impl_widget!(Expander);

impl Expander {
    /// The heading text.
    #[inline]
    pub fn text(&self) -> String {
        self.0.text()
    }

    #[inline]
    pub fn set_text(&self, text: &str) {
        self.0.set_text(text)
    }

    /// Whether it is expanded.
    #[inline]
    pub fn is_expanded(&self) -> bool {
        self.0.is_expanded()
    }

    /// Expands or collapses it from code. `on_toggle` is not called.
    #[inline]
    pub fn set_expanded(&self, expanded: bool) {
        self.0.set_expanded(expanded)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// The contents to collapse. Each call replaces them.
    #[inline]
    pub fn set_child(&self, child: &dyn Widget) {
        self.0.set_child(child.as_backend())
    }

    /// Called with the new state every time the user expands or collapses it.
    #[inline]
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_toggle(f)
    }
}
