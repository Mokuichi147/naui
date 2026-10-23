use crate::sys;
use naui_core::Color;

/// A control for picking a color (`GtkColorDialogButton`).
///
/// Its value right after creation is black ([`Color::BLACK`]).
#[derive(Clone)]
pub struct ColorPicker(pub(crate) sys::ColorPicker);

impl_widget!(ColorPicker);

impl ColorPicker {
    /// The currently selected color.
    #[inline]
    pub fn value(&self) -> Color {
        self.0.value()
    }

    /// Replaces the color from code. `on_change` is not called.
    #[inline]
    pub fn set_value(&self, color: Color) {
        self.0.set_value(color)
    }

    /// Sets the color through the same path as a user's pick and notifies once.
    #[inline]
    pub fn pick(&self, color: Color) {
        self.0.pick(color)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// Called with the new color every time the color changes.
    #[inline]
    pub fn on_change(&self, f: impl FnMut(Color) + 'static) {
        self.0.on_change(f)
    }
}
