use crate::sys;
use naui_core::{EventResponse, KeyEvent, NumberSpec};

/// 数値を入力させるコントロール (`GtkSpinButton`)。
///
/// 既定は整数 (刻み 1、小数桁 0、範囲の制限なし)。
#[derive(Clone)]
pub struct NumberInput(pub(crate) sys::NumberInput);

impl_widget!(NumberInput);

impl NumberInput {
    /// キーが押されたときの通知。`Handled` を返すと、そのキーは
    /// 入力欄にもウィンドウにも渡らない (改行や確定を止められる)。
    ///
    /// IME で変換している間のキーは届かない。
    #[inline]
    pub fn on_key_down(&self, f: impl FnMut(&KeyEvent) -> EventResponse + 'static) {
        self.0.on_key_down(f)
    }

    /// フォーカスが入ったとき (`true`) と出たとき (`false`) の通知。
    ///
    /// ウィンドウの中でフォーカスが移ったときだけ呼ばれる。ウィンドウ
    /// 自体がアクティブでなくなっても呼ばれない。
    #[inline]
    pub fn on_focus_change(&self, f: impl FnMut(bool) + 'static) {
        self.0.on_focus_change(f)
    }

    /// いまの値。
    #[inline]
    pub fn value(&self) -> f64 {
        self.0.value()
    }

    /// 値を通知せずに変える。小数桁へ丸め、範囲の外なら端へ寄せる。
    #[inline]
    pub fn set_value(&self, value: f64) {
        self.0.set_value(value)
    }

    /// 入れられる範囲を決める。`None` はその側に制限を置かない。
    ///
    /// いまの値が範囲から外れていれば、通知せずに端へ寄せる。
    #[inline]
    pub fn set_range(&self, min: Option<f64>, max: Option<f64>) {
        self.0.set_range(min, max)
    }

    /// 上下のボタンやキーで 1 回に動く量 (既定は 1)。
    #[inline]
    pub fn set_step(&self, step: f64) {
        self.0.set_step(step)
    }

    /// 表示する小数の桁数 (既定は 0 = 整数)。
    #[inline]
    pub fn set_decimals(&self, decimals: u32) {
        self.0.set_decimals(decimals)
    }

    /// いまの値の決まり (範囲・刻み・小数桁)。
    #[inline]
    pub fn spec(&self) -> NumberSpec {
        self.0.spec()
    }

    #[inline]
    pub fn set_enabled(&self, enabled: bool) {
        self.0.set_enabled(enabled)
    }

    /// 値が変わったときに、変わったあとの値で呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    #[inline]
    pub fn on_change(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_change(f)
    }

    /// Enter で確定したときに、確定したあとの値で呼ばれる。
    ///
    /// 打ちかけの表示は先に値へ読み込む (値が変われば `on_change` が先に
    /// 呼ばれる)。数として読めない表示は元の値へ戻してから呼ぶ。IME で
    /// 変換している間の Enter では呼ばれない。
    #[inline]
    pub fn on_submit(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_submit(f)
    }
}
