//! 数値入力 (`GtkSpinButton`)。
//!
//! GTK4 には数値専用のコントロールがあるので、[`NumberSpec`] を
//! 範囲 (`GtkAdjustment`)・刻み・桁数へ写すだけで済む。
//!
//! 値の丸めと範囲は naui 側でも持つ。`GtkSpinButton` は表示だけを桁数で
//! 丸め、値そのものは丸めないため、4 バックエンドで同じ答えを返すには
//! [`NumberSpec`] を通した値を持っておく必要がある。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use naui_core::NumberSpec;

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::widgets::{impl_widget, without_signal, Widget};

/// 範囲を指定されていないときに `GtkAdjustment` へ渡す端の値。
///
/// `GtkSpinButton` は下限・上限を必ず持つので、実用上たどり着けない大きさを
/// 「制限なし」の代わりに使う。丸めと範囲は [`NumberSpec`] が持っているので、
/// ここで止まっても naui の答えは変わらない。
const RANGE_LIMIT: f64 = 1.0e15;

/// 数字の欄に見せておく桁数。
///
/// `GtkSpinButton` は幅を決めるとき、範囲に入る数の桁数を数える。範囲を
/// 指定していない欄は [`RANGE_LIMIT`] の 16 桁ぶんを欲しがってしまうので、
/// 「自然な大きさ」はここで決め直す。最小のほうは欄を 0 桁まで許して
/// (`set_width_chars(0)`)、幅を決めるのはアプリに任せる。
const FIELD_CHARS: i32 = 8;

struct NumberInputInner {
    native: gtk::SpinButton,
    bin: SizeBin,
    spec: Cell<NumberSpec>,
    value: Cell<f64>,
    on_change: Notifier<f64>,
    /// プログラムから値を書くときに止めるシグナル。
    value_handler: RefCell<Option<glib::SignalHandlerId>>,
    text_handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// 数値を入力させるコントロール (`GtkSpinButton`)。
///
/// 既定は整数 (刻み 1、小数桁 0、範囲の制限なし)。
#[derive(Clone)]
pub struct NumberInput(Rc<NumberInputInner>);
impl_widget!(NumberInput);

impl NumberInput {
    pub(crate) fn new(value: f64) -> Self {
        let spec = NumberSpec::default();
        let native = gtk::SpinButton::with_range(-RANGE_LIMIT, RANGE_LIMIT, spec.step);
        // 数字以外は受け付けない。
        native.set_numeric(true);
        native.set_width_chars(0);
        native.set_max_width_chars(FIELD_CHARS);
        let bin = SizeBin::wrap(&native);
        // 欄が 0 桁まで縮んでも、上下のボタンのぶんの幅は要る。それより狭い
        // 幅を指定されたときに、ボタンが枠の外へはみ出さないようにする。
        bin.mark_rigid_width();

        let this = Self(Rc::new(NumberInputInner {
            native,
            bin,
            spec: Cell::new(spec),
            value: Cell::new(spec.clamp(value)),
            on_change: Notifier::default(),
            value_handler: RefCell::new(None),
            text_handler: RefCell::new(None),
        }));
        this.write_native_spec();
        this.write_native(this.value());

        // 上下のボタン・確定は `value-changed`、打鍵は `changed` で届く。
        let value_handler = {
            let weak = Rc::downgrade(&this.0);
            this.0.native.connect_value_changed(move |native| {
                if let Some(inner) = weak.upgrade() {
                    NumberInput(inner).native_changed(native.value(), true);
                }
            })
        };
        *this.0.value_handler.borrow_mut() = Some(value_handler);
        let text_handler = {
            let weak = Rc::downgrade(&this.0);
            this.0.native.connect_changed(move |native| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let this = NumberInput(inner);
                // 読めるものだけ受け取る。打ちかけや空欄は確定まで放っておく。
                if let Some(shown) = this.0.spec.get().parse(native.text().as_str()) {
                    this.native_changed(shown, false);
                }
            })
        };
        *this.0.text_handler.borrow_mut() = Some(text_handler);
        this
    }

    /// いまの値。
    pub fn value(&self) -> f64 {
        self.0.value.get()
    }

    /// 値を通知せずに変える。小数桁へ丸め、範囲の外なら端へ寄せる。
    pub fn set_value(&self, value: f64) {
        let value = self.0.spec.get().clamp(value);
        self.0.value.set(value);
        self.write_native(value);
    }

    /// 入れられる範囲を決める。`None` はその側に制限を置かない。
    ///
    /// いまの値が範囲から外れていれば、通知せずに端へ寄せる。
    pub fn set_range(&self, min: Option<f64>, max: Option<f64>) {
        self.update_spec(|spec| spec.range(min, max));
    }

    /// 上下のボタンやキーで 1 回に動く量 (既定は 1)。
    pub fn set_step(&self, step: f64) {
        self.update_spec(|spec| spec.step(step));
    }

    /// 表示する小数の桁数 (既定は 0 = 整数)。
    pub fn set_decimals(&self, decimals: u32) {
        self.update_spec(|spec| spec.decimals(decimals));
    }

    /// いまの値の決まり (範囲・刻み・小数桁)。
    pub fn spec(&self) -> NumberSpec {
        self.0.spec.get()
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// 値が変わったときに、変わったあとの値で呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_change(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_change.set(f);
    }

    /// GTK4 の実コントロール。バックエンド固有の脱出口として公開している。
    pub fn native_spin_button(&self) -> gtk::SpinButton {
        self.0.native.clone()
    }

    /// 画面に出ている値を受け取る。`commit` なら表示も書き直す。
    fn native_changed(&self, shown: f64, commit: bool) {
        let accepted = self.0.spec.get().clamp(shown);
        if commit {
            // 丸めや範囲で押し戻したときのために書き直す
            // (同じ値なら GTK 側は何もしない)。
            self.write_native(accepted);
        }
        if accepted == self.value() {
            return;
        }
        self.0.value.set(accepted);
        self.0.on_change.emit(accepted);
    }

    fn update_spec(&self, edit: impl FnOnce(NumberSpec) -> NumberSpec) {
        self.0.spec.set(edit(self.0.spec.get()));
        self.write_native_spec();
        self.set_value(self.value());
    }

    /// 値をコントロールへ書く。この間はシグナルを止める。
    fn write_native(&self, value: f64) {
        without_signal(&self.0.native, &self.0.value_handler, || {
            without_signal(&self.0.native, &self.0.text_handler, || {
                self.0.native.set_value(value);
            });
        });
    }

    /// 範囲・刻み・桁数を `GtkSpinButton` へ渡す。
    fn write_native_spec(&self) {
        let spec = self.0.spec.get();
        without_signal(&self.0.native, &self.0.value_handler, || {
            without_signal(&self.0.native, &self.0.text_handler, || {
                self.0.native.set_range(
                    spec.min.unwrap_or(-RANGE_LIMIT),
                    spec.max.unwrap_or(RANGE_LIMIT),
                );
                // 2 つめは Page Up / Page Down で動く量。
                self.0.native.set_increments(spec.step, spec.step * 10.0);
                self.0.native.set_digits(spec.decimals);
            });
        });
    }
}
