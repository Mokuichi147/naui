//! 数値入力 (`<input type="number">`)。
//!
//! 上下のボタン (スピナー) と、数字だけを受け付ける入力はブラウザが出す。
//! naui は [`NumberSpec`] を `min` / `max` / `step` 属性へ写し、値の文字列を
//! `f64` と行き来させるだけ。
//!
//! 打っている最中に表示を書き換えると打ちづらいので、**書き戻しは確定
//! (`change`) のときだけ**行う ([`DatePicker`](crate::DatePicker) と同じ)。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{NumberSpec, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlInputElement};

use crate::widgets::{create, impl_widget, Listener, ValueHandler, Widget};

struct NumberInputInner {
    input: HtmlInputElement,
    spec: Cell<NumberSpec>,
    value: Cell<f64>,
    on_change: ValueHandler<f64>,
    /// 入力中と確定時の購読。落とすと購読も外れる。
    listeners: RefCell<Vec<Listener>>,
}

/// 数値を入力させるコントロール (`<input type="number">`)。
///
/// 既定は整数 (刻み 1、小数桁 0、範囲の制限なし)。
#[derive(Clone)]
pub struct NumberInput(Rc<NumberInputInner>);
impl_widget!(NumberInput, input);

impl NumberInput {
    pub(crate) fn new(document: &Document, value: f64) -> Result<Self> {
        let input: HtmlInputElement = create(document, "input")?.unchecked_into();
        input.set_type("number");

        let spec = NumberSpec::default();
        let this = Self(Rc::new(NumberInputInner {
            input,
            spec: Cell::new(spec),
            value: Cell::new(spec.clamp(value)),
            on_change: ValueHandler::default(),
            listeners: RefCell::new(Vec::new()),
        }));
        this.write_native_spec();
        this.write_native(this.value());

        // 打鍵とスピナーは `input`、欄を離れたときの確定は `change`。
        let mut listeners = Vec::new();
        for event in ["input", "change"] {
            let listener = Listener::attach(this.0.input.as_ref(), event, {
                let weak = Rc::downgrade(&this.0);
                let commit = event == "change";
                move || {
                    if let Some(inner) = weak.upgrade() {
                        NumberInput(inner).native_changed(commit);
                    }
                }
            });
            if let Ok(listener) = listener {
                listeners.push(listener);
            }
        }
        *this.0.listeners.borrow_mut() = listeners;
        Ok(this)
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
        self.0.input.set_disabled(!enabled);
    }

    /// 値が変わったときに、変わったあとの値で呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_change(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_change.set(f);
    }

    /// 対応する `<input>`。バックエンド固有の脱出口として公開している。
    pub fn native_input(&self) -> HtmlInputElement {
        self.0.input.clone()
    }

    /// ブラウザ側で値が変わったときの処理。
    ///
    /// `commit` は欄を離れたときの `change` かどうか。
    fn native_changed(&self, commit: bool) {
        let text = self.0.input.value();
        let spec = self.0.spec.get();
        let Some(shown) = spec.parse(&text) else {
            // 数として読めない (空欄や打ちかけ)。確定なら元の値へ戻す。
            if commit {
                self.write_native(self.value());
            }
            return;
        };
        let accepted = spec.clamp(shown);
        if commit && spec.format(accepted) != text {
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

    /// 値を `<input>` へ書く。`value` の代入では `input` も `change` も
    /// 起きないので、購読を止める必要はない。
    fn write_native(&self, value: f64) {
        self.0.input.set_value(&self.0.spec.get().format(value));
    }

    /// 範囲と刻みを属性へ書く。空文字を渡すと属性が外れ、制限なしに戻る。
    fn write_native_spec(&self) {
        let spec = self.0.spec.get();
        let bound = |value: Option<f64>| value.map(|v| v.to_string()).unwrap_or_default();
        self.0.input.set_min(&bound(spec.min));
        self.0.input.set_max(&bound(spec.max));
        self.0.input.set_step(&spec.step.to_string());
    }
}
