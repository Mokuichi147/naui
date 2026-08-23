//! 数値入力の値の決まり (下限・上限・刻み・小数桁)。
//!
//! 数値の欄は 4 環境とも「文字列を持つ入力欄 + 上下のボタン」で、値の丸めや
//! 範囲の当て方はコントロールごとに違う。バックエンドごとにそれを書くと
//! 答えがずれるので、[`DatePickerMode`](crate::DatePickerMode) と同じく
//! 値の扱いだけをここに置き、4 バックエンドで共有する。

/// 数値入力が受け付ける値の決まり。
///
/// 既定は**整数**の入力 (刻み 1、小数桁 0、範囲の制限なし)。小数を扱うときは
/// 刻みと小数桁の両方を指定する。
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
    /// 下限。`None` はその側に制限を置かない。
    pub min: Option<f64>,
    /// 上限。`None` はその側に制限を置かない。
    pub max: Option<f64>,
    /// 上下のボタンやキーで 1 回に動く量。
    pub step: f64,
    /// 表示する小数の桁数。
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
    /// [`format`](Self::format) が扱える小数桁の上限。これを超える指定は
    /// ここまで切り詰める (`f64` はこれ以上の桁を区別できないため)。
    pub const MAX_DECIMALS: u32 = 15;

    /// 範囲を差し替えた値を返す。
    pub fn range(mut self, min: Option<f64>, max: Option<f64>) -> Self {
        self.min = min.filter(|v| v.is_finite());
        self.max = max.filter(|v| v.is_finite());
        self
    }

    /// 刻みを差し替えた値を返す。0 以下や有限でない値は 1 として扱う。
    pub fn step(mut self, step: f64) -> Self {
        self.step = if step.is_finite() && step > 0.0 {
            step
        } else {
            1.0
        };
        self
    }

    /// 小数桁を差し替えた値を返す。[`MAX_DECIMALS`](Self::MAX_DECIMALS) まで。
    pub fn decimals(mut self, decimals: u32) -> Self {
        self.decimals = decimals.min(Self::MAX_DECIMALS);
        self
    }

    /// 値を**小数桁へ丸めてから**下限・上限へ収める。
    ///
    /// 丸めを先に行うのは、表示に出る桁と `value()` の返す値を同じにするため。
    /// 端そのものは丸めないので、範囲の端が小数桁より細かければその値が返る。
    /// 下限が上限より大きいときは、あとから当てる上限が勝つ
    /// ([`DatePickerMode::clamp`](crate::DatePickerMode::clamp) と同じ)。
    /// 数として読めない値 (`NaN` や無限) は 0 として扱う。
    ///
    /// ```
    /// # use naui_core::NumberSpec;
    /// let spec = NumberSpec::default().decimals(1).range(Some(0.05), None);
    /// assert_eq!(spec.clamp(1.44), 1.4);
    /// assert_eq!(spec.clamp(0.0), 0.05, "端は丸めずにそのまま返る");
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

    /// 上下のボタンを `steps` 回押したときの値。範囲の外へは出ない。
    pub fn stepped(self, value: f64, steps: f64) -> f64 {
        self.clamp(value + self.step * steps)
    }

    /// 画面に出す文字列。小数桁は必ずこの桁数まで書く。
    ///
    /// ```
    /// # use naui_core::NumberSpec;
    /// assert_eq!(NumberSpec::default().decimals(2).format(1.5), "1.50");
    /// assert_eq!(NumberSpec::default().format(-0.4), "-0", "丸めは表示だけ");
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

    /// [`format`](Self::format) の逆。数として読めない文字列では `None`。
    ///
    /// 前後の空白は無視する。空欄や `NaN` / `inf` は読めなかった扱いにする
    /// (入力の途中で欄が空になることがあるため)。
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

    /// 小数桁へ丸める。桁が大きすぎて丸められないときは元の値を返す。
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
