use crate::sys;
use naui_core::{EventResponse, KeyEvent, NumberSpec};

/// A control for entering a number (`GtkSpinButton`).
///
/// The default is integers (step 1, 0 decimal places, no range limit).
#[derive(Clone)]
pub struct NumberInput(pub(crate) sys::NumberInput);

impl_widget!(NumberInput);

impl NumberInput {
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

    /// The current value.
    #[inline]
    pub fn value(&self) -> f64 {
        self.0.value()
    }

    /// Changes the value without notifying. Rounds to the decimal places and clamps to
    /// the range.
    #[inline]
    pub fn set_value(&self, value: f64) {
        self.0.set_value(value)
    }

    /// Sets the range of accepted values. `None` places no limit on that side.
    ///
    /// If the current value falls outside the range, it is clamped to the end without
    /// notifying.
    #[inline]
    pub fn set_range(&self, min: Option<f64>, max: Option<f64>) {
        self.0.set_range(min, max)
    }

    /// How much one press of the up/down buttons or keys moves the value (default 1).
    #[inline]
    pub fn set_step(&self, step: f64) {
        self.0.set_step(step)
    }

    /// The number of decimal places shown (default 0 = integers).
    #[inline]
    pub fn set_decimals(&self, decimals: u32) {
        self.0.set_decimals(decimals)
    }

    /// The current value rules (range, step, decimal places).
    #[inline]
    pub fn spec(&self) -> NumberSpec {
        self.0.spec()
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// Called with the new value when the value changes. Setting it again removes the
    /// previous callback.
    #[inline]
    pub fn on_change(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_change(f)
    }

    /// Called with the confirmed value when the user confirms with Enter.
    ///
    /// What has been typed so far is read into the value first (if the value changes,
    /// `on_change` is called first). Text that cannot be read as a number is reverted
    /// to the previous value before the call. Not called for an Enter pressed while an
    /// IME is composing.
    #[inline]
    pub fn on_submit(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_submit(f)
    }

    /// Replaces the text in its field with `text` the way the user does by
    /// selecting everything and typing over it. Does nothing while disabled.
    ///
    /// As with typing, text that reads as a number is taken as the value at once
    /// (rounded and kept in range only on confirmation), so
    /// [`on_change`](Self::on_change) is called if the value changes. Other text,
    /// such as `"-"`, changes nothing until [`submit`](Self::submit) or leaving the
    /// field confirms it. What the field keeps of such text is the platform's own:
    /// the Web and Linux drop characters a number cannot have.
    ///
    /// For tests and automation. It goes through each platform's own path for
    /// typed text; no key is pressed, so `on_key_down` is not called. See
    /// [Testing apps](crate::guide#testing-apps).
    #[inline]
    pub fn enter_text(&self, text: &str) {
        self.0.enter_text(text)
    }

    /// Confirms it as Enter does: the text in the field is read into the value
    /// (or reverted if it is not a number), and [`on_submit`](Self::on_submit) is
    /// called. Does nothing while disabled.
    ///
    /// For tests and automation. No key is pressed, so `on_key_down` is not
    /// called. See [Testing apps](crate::guide#testing-apps).
    #[inline]
    pub fn submit(&self) {
        self.0.submit()
    }
}
