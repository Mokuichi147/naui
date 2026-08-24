//! トースト (一時的な通知) の値型。
//!
//! トーストは「短い文字列と、任意の操作ボタンを、しばらく出してから自分で
//! 消す」通知で、ネイティブのものがあるのは libadwaita の `AdwToast` だけ。
//! 残る 3 環境は naui が同じ形に組み立てるため、**何をどれだけの間出すか**は
//! [`NumberSpec`](crate::NumberSpec) と同じくここへ置き、4 バックエンドで
//! 共有する。

/// トーストに出す内容と、消えるまでの時間。
///
/// 既定は「5 秒で消える、操作ボタンの無いトースト」。
///
/// ```
/// # use naui_core::ToastSpec;
/// let mut spec = ToastSpec::new("保存しました");
/// assert_eq!(spec.timeout(), ToastSpec::DEFAULT_TIMEOUT);
///
/// spec.set_action("元に戻す");
/// assert_eq!(spec.action(), Some("元に戻す"));
///
/// spec.set_timeout(0.0); // 0 は「自動では消えない」
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
    /// 何も指定しなかったときに消えるまでの秒数。`AdwToast` の既定と同じ。
    pub const DEFAULT_TIMEOUT: f64 = 5.0;

    /// 受け付ける最長の秒数 (1 日)。これより長い指定はここまで切り詰める。
    ///
    /// 上限を置くのは、ブラウザの `setTimeout` が 32 ビットのミリ秒しか
    /// 受け取らないため。1 日出しっぱなしのトーストは、実質
    /// [`is_persistent`](Self::is_persistent) と変わらない。
    pub const MAX_TIMEOUT: f64 = 86_400.0;

    /// 出す文字列を決めて作る。
    pub fn new(message: &str) -> Self {
        Self {
            message: message.to_string(),
            ..Self::default()
        }
    }

    /// 出す文字列。
    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn set_message(&mut self, message: &str) {
        self.message = message.to_string();
    }

    /// 操作ボタンの文字列。置いていなければ `None`。
    pub fn action(&self) -> Option<&str> {
        self.action.as_deref()
    }

    /// 操作ボタンの文字列。**空文字列を渡すとボタンを外す**
    /// ([`DialogButtons`](crate::DialogButtons) と同じ)。
    pub fn set_action(&mut self, label: &str) {
        self.action = if label.is_empty() {
            None
        } else {
            Some(label.to_string())
        };
    }

    /// 消えるまでの秒数。0 なら自動では消えない。
    pub fn timeout(&self) -> f64 {
        self.timeout
    }

    /// 消えるまでの秒数。
    ///
    /// **0 以下と、数として読めない値 (`NaN` や無限) は 0** として扱う。
    /// 0 は「自動では消えない」で、`dismiss()` か操作ボタンでしか消えなく
    /// なる (`AdwToast` の `timeout` と同じ決まり)。
    /// [`MAX_TIMEOUT`](Self::MAX_TIMEOUT) より長い指定はそこまで切り詰める。
    ///
    /// ```
    /// # use naui_core::ToastSpec;
    /// let mut spec = ToastSpec::new("完了");
    /// spec.set_timeout(-1.0);
    /// assert_eq!(spec.timeout(), 0.0, "負の指定は「消えない」にそろえる");
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

    /// 自動では消えないか。
    pub fn is_persistent(&self) -> bool {
        self.timeout <= 0.0
    }

    /// 消えるまでのミリ秒。自動で消えないなら `None`。
    ///
    /// ブラウザの `setTimeout` と WinUI の `DispatcherQueueTimer` へ渡す。
    ///
    /// ```
    /// # use naui_core::ToastSpec;
    /// let mut spec = ToastSpec::new("完了");
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

    /// 消えるまでの秒数を整数で。自動で消えないなら 0。
    ///
    /// `adw_toast_set_timeout` が秒でしか受け取らないため、**1 秒未満の指定は
    /// 1 秒になる**。0 へ丸めてしまうと、GTK では「消えないトースト」に
    /// 変わってしまうため。
    ///
    /// ```
    /// # use naui_core::ToastSpec;
    /// let mut spec = ToastSpec::new("完了");
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
