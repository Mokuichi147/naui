use crate::sys;

/// An on/off switch (`GtkSwitch`).
#[derive(Clone)]
pub struct Toggle(pub(crate) sys::Toggle);

impl_widget!(Toggle);

impl Toggle {
    /// Whether it is on.
    #[inline]
    pub fn is_on(&self) -> bool {
        self.0.is_on()
    }

    /// Switches it from code. `on_toggle` is not called.
    #[inline]
    pub fn set_on(&self, on: bool) {
        self.0.set_on(on)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// Called with the new state every time the user switches it.
    #[inline]
    pub fn on_toggle(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_toggle(f)
    }
}
