use crate::sys;
use naui_core::{DatePickerMode, DateTime};

/// A control for picking a date and time.
///
/// Its value right after creation is the platform's current date and time (local
/// time).
#[derive(Clone)]
pub struct DatePicker(pub(crate) sys::DatePicker);

impl_widget!(DatePicker);

impl DatePicker {
    /// What it lets the user pick. Fixed at creation and never changes.
    #[inline]
    pub fn mode(&self) -> DatePickerMode {
        self.0.mode()
    }

    /// The current value. Parts that cannot be picked are returned as they were passed.
    #[inline]
    pub fn value(&self) -> DateTime {
        self.0.value()
    }

    /// Changes the value without notifying. Out-of-range values are clamped to the
    /// ends, and values that are not valid dates are normalized.
    #[inline]
    pub fn set_value(&self, value: DateTime) {
        self.0.set_value(value)
    }

    /// Sets the selectable range. `None` places no limit on that side.
    ///
    /// If the current value falls outside the range, it is clamped to the end without
    /// notifying. Neither `GtkCalendar` nor `GtkSpinButton` has a "calendar range", so
    /// the range is enforced only by naui's clamping.
    #[inline]
    pub fn set_range(&self, min: Option<DateTime>, max: Option<DateTime>) {
        self.0.set_range(min, max)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// Called with the new value when the value changes. Setting it again removes the
    /// previous callback.
    #[inline]
    pub fn on_change(&self, f: impl FnMut(DateTime) + 'static) {
        self.0.on_change(f)
    }
}
