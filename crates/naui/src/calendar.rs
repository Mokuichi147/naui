use crate::sys;
use naui_core::DateTime;

/// A month calendar that stays on screen and lets the user pick one day.
///
/// The value is a [`DateTime`] of which only the year, month and day are picked, the
/// same as [`DatePicker`](crate::DatePicker) in [`DatePickerMode::Date`]: the time
/// part is kept as it was passed. A day is always selected. The calendar is the
/// Gregorian one, and the month and weekday names, the first day of the week and
/// the layout follow the platform's locale.
///
/// Its value right after creation is the platform's current date and time (local
/// time), and it shows that month.
///
/// [`DatePickerMode::Date`]: crate::DatePickerMode::Date
#[derive(Clone)]
pub struct Calendar(pub(crate) sys::Calendar);

impl_widget!(Calendar);

impl Calendar {
    /// The selected date. The time part is returned as it was passed.
    #[inline]
    pub fn value(&self) -> DateTime {
        self.0.value()
    }

    /// Selects a date without notifying, and shows its month. Out-of-range dates are
    /// clamped to the ends, and values that are not valid dates are normalized.
    #[inline]
    pub fn set_value(&self, value: DateTime) {
        self.0.set_value(value)
    }

    /// Sets the selectable range of days. `None` places no limit on that side. Only
    /// the dates are compared; the times of `min` and `max` are ignored.
    ///
    /// If the current value falls outside the range, it is clamped to the end without
    /// notifying.
    #[inline]
    pub fn set_range(&self, min: Option<DateTime>, max: Option<DateTime>) {
        self.0.set_range(min, max)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// Called with the new value when the user selects another day. Setting it again
    /// removes the previous callback.
    #[inline]
    pub fn on_change(&self, f: impl FnMut(DateTime) + 'static) {
        self.0.on_change(f)
    }

    /// Selects the day of `value` the way the user does by clicking it: the
    /// calendar shows its month, the day is selected, and
    /// [`on_change`](Self::on_change) is called with the new value (the time part
    /// stays as it was). A date outside the range is clamped to the end first.
    /// Nothing is called when the day is already selected. Does nothing while
    /// disabled.
    ///
    /// For tests and automation. The notification arrives before this returns. See
    /// [Testing apps](crate::guide#testing-apps).
    #[inline]
    pub fn pick(&self, value: DateTime) {
        self.0.pick(value)
    }
}
