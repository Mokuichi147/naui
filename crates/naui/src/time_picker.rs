use crate::sys;
use naui_core::Time;

/// A control for picking a time of day (`GtkSpinButton`s for hours and minutes).
///
/// Its value right after creation is the platform's current time (local time).
#[derive(Clone)]
pub struct TimePicker(pub(crate) sys::TimePicker);

impl_widget!(TimePicker);

impl TimePicker {
    /// The currently selected time.
    #[inline]
    pub fn value(&self) -> Time {
        self.0.value()
    }

    /// Changes the value without notifying. Out-of-range values are clamped to the
    /// ends, and values that are not valid times are normalized.
    #[inline]
    pub fn set_value(&self, value: Time) {
        self.0.set_value(value)
    }

    /// Sets the selectable range. `None` places no limit on that side.
    ///
    /// If the current value falls outside the range, it is clamped to the end without
    /// notifying.
    #[inline]
    pub fn set_range(&self, min: Option<Time>, max: Option<Time>) {
        self.0.set_range(min, max)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// Called with the new value when the value changes. Setting it again removes the
    /// previous callback.
    #[inline]
    pub fn on_change(&self, f: impl FnMut(Time) + 'static) {
        self.0.on_change(f)
    }
}
