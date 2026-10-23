//! Value rules for number input (minimum, maximum, step, decimal places).
//!
//! On all four platforms a number field is "a text field + up/down buttons", and
//! how values are rounded and how ranges are applied differ per control. Writing
//! that in each backend would make the answers drift, so, as with
//! [`DatePickerMode`](crate::DatePickerMode), only the value handling lives here
//! and is shared by all four backends.

/// The rules for values a number input accepts.
///
/// The default is **integer** input (step 1, 0 decimal places, no range limit). To
/// handle decimals, set both the step and the decimal places.
///
/// ```
/// # use naui_core::NumberSpec;
/// let spec = NumberSpec::default().range(Some(0.0), Some(10.0));
/// assert_eq!(spec.clamp(12.4), 10.0);
/// assert_eq!(spec.format(3.7), "4");
/// assert_eq!(spec.stepped(9.5, 1.0), 10.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumberSpec {
    /// The minimum. `None` places no limit on that side.
    pub min: Option<f64>,
    /// The maximum. `None` places no limit on that side.
    pub max: Option<f64>,
    /// How much one press of the up/down buttons or keys moves the value.
    pub step: f64,
    /// The number of decimal places shown.
    pub decimals: u32,
}

impl Default for NumberSpec {
    fn default() -> Self {
        Self {
            min: None,
            max: None,
            step: 1.0,
            decimals: 0,
        }
    }
}

impl NumberSpec {
    /// The maximum number of decimal places [`format`](Self::format) handles. Larger
    /// values are truncated to this (`f64` cannot distinguish more digits).
    pub const MAX_DECIMALS: u32 = 15;

    /// Returns the spec with the range replaced.
    pub fn range(mut self, min: Option<f64>, max: Option<f64>) -> Self {
        self.min = min.filter(|v| v.is_finite());
        self.max = max.filter(|v| v.is_finite());
        self
    }

    /// Returns the spec with the step replaced. Values of 0 or less, or non-finite
    /// values, are treated as 1.
    pub fn step(mut self, step: f64) -> Self {
        self.step = if step.is_finite() && step > 0.0 {
            step
        } else {
            1.0
        };
        self
    }

    /// Returns the spec with the decimal places replaced. Up to
    /// [`MAX_DECIMALS`](Self::MAX_DECIMALS).
    pub fn decimals(mut self, decimals: u32) -> Self {
        self.decimals = decimals.min(Self::MAX_DECIMALS);
        self
    }

    /// **Rounds the value to the decimal places first**, then clamps it between the
    /// minimum and maximum.
    ///
    /// Rounding comes first so that the digits on screen match what `value()` returns.
    /// The bounds themselves are not rounded, so if a bound is finer than the decimal
    /// places, that value is returned. When the minimum is larger than the maximum, the
    /// maximum, applied last, wins (the same as
    /// [`DatePickerMode::clamp`](crate::DatePickerMode::clamp)). Values that are not
    /// numbers (`NaN` and infinities) are treated as 0.
    ///
    /// ```
    /// # use naui_core::NumberSpec;
    /// let spec = NumberSpec::default().decimals(1).range(Some(0.05), None);
    /// assert_eq!(spec.clamp(1.44), 1.4);
    /// assert_eq!(spec.clamp(0.0), 0.05, "bounds are returned as is, without rounding");
    /// ```
    pub fn clamp(self, value: f64) -> f64 {
        let mut out = self.round(if value.is_finite() { value } else { 0.0 });
        if let Some(min) = self.min {
            if out < min {
                out = min;
            }
        }
        if let Some(max) = self.max {
            if out > max {
                out = max;
            }
        }
        out
    }

    /// The value after pressing the up/down buttons `steps` times. It never leaves the
    /// range.
    pub fn stepped(self, value: f64, steps: f64) -> f64 {
        self.clamp(value + self.step * steps)
    }

    /// The text shown on screen. Always written with exactly this many decimal places.
    ///
    /// ```
    /// # use naui_core::NumberSpec;
    /// assert_eq!(NumberSpec::default().decimals(2).format(1.5), "1.50");
    /// assert_eq!(NumberSpec::default().format(-0.4), "-0", "rounding affects only the display");
    /// ```
    pub fn format(self, value: f64) -> String {
        let value = if value.is_finite() { value } else { 0.0 };
        // `-0.0` は `-0` と書かれてしまうので、0 は符号を落とす。
        let value = if value == 0.0 { 0.0 } else { value };
        format!(
            "{:.*}",
            self.decimals.min(Self::MAX_DECIMALS) as usize,
            value
        )
    }

    /// The inverse of [`format`](Self::format). `None` for strings that cannot be read
    /// as a number.
    ///
    /// Leading and trailing whitespace is ignored. Empty fields and `NaN` / `inf` count
    /// as unreadable (the field can be empty mid-edit).
    ///
    /// ```
    /// # use naui_core::NumberSpec;
    /// let spec = NumberSpec::default();
    /// assert_eq!(spec.parse(" 12.5 "), Some(12.5));
    /// assert_eq!(spec.parse(""), None);
    /// assert_eq!(spec.parse("inf"), None);
    /// ```
    pub fn parse(self, text: &str) -> Option<f64> {
        text.trim().parse::<f64>().ok().filter(|v| v.is_finite())
    }

    /// Rounds to the decimal places. Returns the original value when there are too
    /// many digits to round.
    fn round(self, value: f64) -> f64 {
        let scale = 10f64.powi(self.decimals.min(Self::MAX_DECIMALS) as i32);
        let rounded = (value * scale).round() / scale;
        if rounded.is_finite() {
            rounded
        } else {
            value
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_an_integer_input() {
        let spec = NumberSpec::default();
        assert_eq!(spec.step, 1.0);
        assert_eq!(spec.decimals, 0);
        assert_eq!(spec.clamp(2.5), 3.0, "0 桁へ丸める");
        assert_eq!(spec.clamp(-2.5), -3.0);
        assert_eq!(spec.format(3.0), "3");
    }

    #[test]
    fn clamp_rounds_then_applies_the_range() {
        let spec = NumberSpec::default()
            .decimals(2)
            .range(Some(-1.0), Some(1.0));
        assert_eq!(spec.clamp(0.129), 0.13);
        assert_eq!(spec.clamp(9.0), 1.0);
        assert_eq!(spec.clamp(-9.0), -1.0);
    }

    #[test]
    fn the_max_wins_over_a_larger_min() {
        let spec = NumberSpec::default().range(Some(10.0), Some(0.0));
        assert_eq!(spec.clamp(5.0), 0.0);
    }

    #[test]
    fn broken_values_become_zero() {
        let spec = NumberSpec::default();
        assert_eq!(spec.clamp(f64::NAN), 0.0);
        assert_eq!(spec.clamp(f64::INFINITY), 0.0);
        assert_eq!(spec.format(f64::NAN), "0");
    }

    #[test]
    fn steps_stay_inside_the_range() {
        let spec = NumberSpec::default()
            .step(0.5)
            .decimals(1)
            .range(None, Some(1.0));
        assert_eq!(spec.stepped(0.0, 1.0), 0.5);
        assert_eq!(spec.stepped(0.0, -1.0), -0.5);
        assert_eq!(spec.stepped(0.9, 1.0), 1.0, "上限で止まる");
    }

    #[test]
    fn a_step_of_zero_falls_back_to_one() {
        assert_eq!(NumberSpec::default().step(0.0).step, 1.0);
        assert_eq!(NumberSpec::default().step(f64::NAN).step, 1.0);
        assert_eq!(NumberSpec::default().step(2.5).step, 2.5);
    }

    #[test]
    fn an_infinite_bound_is_no_bound() {
        let spec = NumberSpec::default().range(Some(f64::NEG_INFINITY), Some(f64::NAN));
        assert_eq!(spec.min, None);
        assert_eq!(spec.max, None);
    }

    #[test]
    fn decimals_are_capped() {
        let spec = NumberSpec::default().decimals(99);
        assert_eq!(spec.decimals, NumberSpec::MAX_DECIMALS);
        assert_eq!(
            spec.format(1.0).len(),
            2 + NumberSpec::MAX_DECIMALS as usize
        );
    }

    #[test]
    fn text_round_trips_through_format_and_parse() {
        let spec = NumberSpec::default().decimals(3);
        assert_eq!(spec.parse(&spec.format(1.25)), Some(1.25));
        assert_eq!(spec.parse("+5"), Some(5.0));
        assert_eq!(spec.parse("１"), None, "全角は読まない");
        assert_eq!(spec.parse("12ab"), None);
    }
}
