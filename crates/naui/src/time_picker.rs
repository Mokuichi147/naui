use crate::sys;
use naui_core::Time;

/// 時刻を選ばせるコントロール (時と分の `GtkSpinButton`)。
///
/// 作った直後の値は、その環境の現在時刻 (ローカル時刻)。
#[derive(Clone)]
pub struct TimePicker(pub(crate) sys::TimePicker);

impl_widget!(TimePicker);

impl TimePicker {
    /// いま選ばれている時刻。
    #[inline]
    pub fn value(&self) -> Time {
        self.0.value()
    }

    /// 値を通知せずに変える。範囲外なら端へ寄せ、時計として成り立たない値は丸める。
    #[inline]
    pub fn set_value(&self, value: Time) {
        self.0.set_value(value)
    }

    /// 選べる範囲を決める。`None` はその側に制限を置かない。
    ///
    /// いまの値が範囲から外れていれば、通知せずに端へ寄せる。
    #[inline]
    pub fn set_range(&self, min: Option<Time>, max: Option<Time>) {
        self.0.set_range(min, max)
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// 値が変わったときに、変わったあとの値で呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    #[inline]
    pub fn on_change(&self, f: impl FnMut(Time) + 'static) {
        self.0.on_change(f)
    }
}
