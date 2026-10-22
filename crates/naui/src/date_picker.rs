use crate::sys;
use naui_core::{DatePickerMode, DateTime};

/// 日付と時刻を選ばせるコントロール。
///
/// 作った直後の値は、その環境の現在日時 (ローカル時刻)。
#[derive(Clone)]
pub struct DatePicker(pub(crate) sys::DatePicker);

impl_widget!(DatePicker);

impl DatePicker {
    /// 何を選ばせるか。生成時に決まり、あとから変わらない。
    #[inline]
    pub fn mode(&self) -> DatePickerMode {
        self.0.mode()
    }

    /// 現在の値。選ばせていない部分も、渡された値のまま返る。
    #[inline]
    pub fn value(&self) -> DateTime {
        self.0.value()
    }

    /// 値を通知せずに変える。範囲外なら端へ寄せ、暦として成り立たない値は丸める。
    #[inline]
    pub fn set_value(&self, value: DateTime) {
        self.0.set_value(value)
    }

    /// 選べる範囲を決める。`None` はその側に制限を置かない。
    ///
    /// いまの値が範囲から外れていれば、通知せずに端へ寄せる。
    /// `GtkCalendar` にも `GtkSpinButton` にも「暦としての範囲」は無いので、
    /// 範囲は naui 側の丸めだけで守る。
    #[inline]
    pub fn set_range(&self, min: Option<DateTime>, max: Option<DateTime>) {
        self.0.set_range(min, max)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// 値が変わったときに、変わったあとの値で呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    #[inline]
    pub fn on_change(&self, f: impl FnMut(DateTime) + 'static) {
        self.0.on_change(f)
    }
}
