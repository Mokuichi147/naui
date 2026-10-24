//! Value types for toasts (transient notifications).
//!
//! A toast is a notification that "shows a short string and an optional action
//! button for a while, then goes away by itself". Only libadwaita has a native one,
//! `AdwToast`. naui builds the same shape on the other three platforms, so **what
//! to show and for how long** lives here, like [`NumberSpec`](crate::NumberSpec),
//! and is shared by all four backends.

/// What a toast shows and how long until it disappears.
///
/// The default is "a toast that disappears after 5 seconds, without an action
/// button".
///
/// ```
/// # use naui_core::ToastSpec;
/// let mut spec = ToastSpec::new("Saved");
/// assert_eq!(spec.timeout(), ToastSpec::DEFAULT_TIMEOUT);
///
/// spec.set_action("Undo");
/// assert_eq!(spec.action(), Some("Undo"));
///
/// spec.set_timeout(0.0); // 0 means "never disappears automatically"
/// assert!(spec.is_persistent());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ToastSpec {
    message: String,
    action: Option<String>,
    timeout: f64,
}

impl Default for ToastSpec {
    fn default() -> Self {
        Self {
            message: String::new(),
            action: None,
            timeout: Self::DEFAULT_TIMEOUT,
        }
    }
}

impl ToastSpec {
    /// Seconds until it disappears when nothing is specified. The same as `AdwToast`'s
    /// default.
    pub const DEFAULT_TIMEOUT: f64 = 5.0;

    /// The longest accepted duration in seconds (one day). Longer values are truncated
    /// to this.
    ///
    /// The cap exists because the browser's `setTimeout` accepts only 32-bit
    /// milliseconds. A toast shown for a whole day is effectively the same as
    /// [`is_persistent`](Self::is_persistent).
    pub const MAX_TIMEOUT: f64 = 86_400.0;

    /// Creates one with the string to show.
    pub fn new(message: &str) -> Self {
        Self {
            message: message.to_string(),
            ..Self::default()
        }
    }

    /// The string to show.
    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn set_message(&mut self, message: &str) {
        self.message = message.to_string();
    }

    /// The action button's label. `None` if there is no button.
    pub fn action(&self) -> Option<&str> {
        self.action.as_deref()
    }

    /// The action button's label. **Passing an empty string removes the button** (the
    /// same as [`DialogButtons`](crate::DialogButtons)).
    pub fn set_action(&mut self, label: &str) {
        self.action = if label.is_empty() {
            None
        } else {
            Some(label.to_string())
        };
    }

    /// Seconds until it disappears. 0 means it never disappears automatically.
    pub fn timeout(&self) -> f64 {
        self.timeout
    }

    /// Seconds until it disappears.
    ///
    /// **Values of 0 or less, and values that are not numbers (`NaN` and infinities),
    /// are treated as 0.** 0 means "never disappears automatically": it goes away only
    /// through `dismiss()` or the action button (the same rule as `AdwToast`'s
    /// `timeout`). Values longer than [`MAX_TIMEOUT`](Self::MAX_TIMEOUT) are truncated
    /// to it.
    ///
    /// ```
    /// # use naui_core::ToastSpec;
    /// let mut spec = ToastSpec::new("Done");
    /// spec.set_timeout(-1.0);
    /// assert_eq!(spec.timeout(), 0.0, "negative values become \"never disappears\"");
    /// spec.set_timeout(f64::INFINITY);
    /// assert_eq!(spec.timeout(), 0.0);
    /// ```
    pub fn set_timeout(&mut self, seconds: f64) {
        self.timeout = if seconds.is_finite() && seconds > 0.0 {
            seconds.min(Self::MAX_TIMEOUT)
        } else {
            0.0
        };
    }

    /// Whether it never disappears automatically.
    pub fn is_persistent(&self) -> bool {
        self.timeout <= 0.0
    }

    /// Milliseconds until it disappears. `None` if it never disappears automatically.
    ///
    /// Passed to the browser's `setTimeout` and WinUI's `DispatcherQueueTimer`.
    ///
    /// ```
    /// # use naui_core::ToastSpec;
    /// let mut spec = ToastSpec::new("Done");
    /// assert_eq!(spec.timeout_millis(), Some(5_000));
    /// spec.set_timeout(0.0);
    /// assert_eq!(spec.timeout_millis(), None);
    /// ```
    pub fn timeout_millis(&self) -> Option<i32> {
        if self.is_persistent() {
            return None;
        }
        // 上限が 1 日なので、ミリ秒にしても i32 に収まる。
        Some((self.timeout * 1_000.0).round().max(1.0) as i32)
    }

    /// Seconds until it disappears, as an integer. 0 if it never disappears
    /// automatically.
    ///
    /// `adw_toast_set_timeout` accepts only whole seconds, so **anything under one
    /// second becomes one second**. Rounding to 0 would turn it into a "toast that
    /// never disappears" on GTK.
    ///
    /// ```
    /// # use naui_core::ToastSpec;
    /// let mut spec = ToastSpec::new("Done");
    /// spec.set_timeout(0.2);
    /// assert_eq!(spec.timeout_secs(), 1);
    /// ```
    pub fn timeout_secs(&self) -> u32 {
        if self.is_persistent() {
            return 0;
        }
        self.timeout.round().max(1.0) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shows_a_plain_toast_for_five_seconds() {
        let spec = ToastSpec::default();
        assert_eq!(spec.message(), "");
        assert_eq!(spec.action(), None);
        assert_eq!(spec.timeout(), 5.0);
        assert!(!spec.is_persistent());
    }

    #[test]
    fn empty_label_removes_the_action_button() {
        let mut spec = ToastSpec::new("保存しました");
        spec.set_action("元に戻す");
        assert_eq!(spec.action(), Some("元に戻す"));
        spec.set_action("");
        assert_eq!(spec.action(), None);
    }

    #[test]
    fn unusable_timeouts_become_persistent() {
        let mut spec = ToastSpec::new("完了");
        for seconds in [0.0, -3.0, f64::NAN, f64::NEG_INFINITY, f64::INFINITY] {
            spec.set_timeout(seconds);
            assert!(spec.is_persistent(), "{seconds} は「消えない」になる");
            assert_eq!(spec.timeout_millis(), None);
            assert_eq!(spec.timeout_secs(), 0);
        }
    }

    #[test]
    fn long_timeouts_are_cut_to_a_day() {
        let mut spec = ToastSpec::new("完了");
        spec.set_timeout(f64::MAX);
        assert_eq!(spec.timeout(), ToastSpec::MAX_TIMEOUT);
        assert_eq!(spec.timeout_millis(), Some(86_400_000));
    }

    #[test]
    fn short_timeouts_keep_at_least_one_unit() {
        let mut spec = ToastSpec::new("完了");
        spec.set_timeout(0.000_1);
        // ミリ秒でも秒でも、0 (= 消えない) には落とさない。
        assert_eq!(spec.timeout_millis(), Some(1));
        assert_eq!(spec.timeout_secs(), 1);
    }
}
