//! 数値入力 (`NSTextField` + `NSStepper`)。
//!
//! AppKit に数値専用のコントロールは無い。数字を打つ欄と上下のボタンを
//! 並べるのが AppKit の標準の組み方 (システム設定の数値欄と同じ形) なので、
//! `NSTextField` と `NSStepper` を `NSStackView` へ並べる。
//!
//! 値の丸めと範囲は [`NumberSpec`] が決める。打っている最中に表示を
//! 書き換えると打ちづらいので、**書き戻しは確定時 (Enter・欄を離れたとき) と
//! 上下のボタンを押したときだけ**行う。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::NumberSpec;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{sel, MainThreadMarker, Message};
use objc2_app_kit::{
    NSLayoutConstraint, NSLayoutConstraintOrientation, NSLayoutPriority, NSStackView,
    NSStackViewDistribution, NSStepper, NSTextAlignment, NSTextField,
    NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::{NSArray, NSString};

use crate::trampoline::{ActionTarget, TextObserver, ValueHandler};
use crate::widgets::{impl_widget, Widget};

/// 数字の欄の最小幅 (論理ピクセル)。中身に合わせると 1 桁分まで縮むため。
const FIELD_MIN_WIDTH: f64 = 72.0;

/// 範囲を指定されていないときに `NSStepper` へ渡す端の値。
///
/// `NSStepper` は下限・上限を必ず持つので、実用上たどり着けない大きさを
/// 「制限なし」の代わりに使う。丸めと範囲は [`NumberSpec`] が持っているので、
/// ここで止まっても naui の答えは変わらない。
const STEPPER_LIMIT: f64 = 1.0e15;

/// 上下のボタンは自分の大きさのままでいて、幅の余りは欄へ渡す。
const STEPPER_HUGGING: NSLayoutPriority = 1000.0;

struct NumberInputInner {
    native: Retained<NSStackView>,
    field: Retained<NSTextField>,
    stepper: Retained<NSStepper>,
    spec: Cell<NumberSpec>,
    value: Cell<f64>,
    handler: ValueHandler<f64>,
    /// 値を書き込んでいる間だけ、ネイティブからの通知を無視する。
    silent: Cell<bool>,
    /// AppKit の target とデリゲートは weak なので、ここで生かしておく。
    observer: RefCell<Option<Retained<TextObserver>>>,
    targets: RefCell<Vec<Retained<ActionTarget>>>,
}

/// 数値を入力させるコントロール (`NSTextField` + `NSStepper`)。
///
/// 既定は整数 (刻み 1、小数桁 0、範囲の制限なし)。
#[derive(Clone)]
pub struct NumberInput(Rc<NumberInputInner>);
impl_widget!(NumberInput);

impl NumberInput {
    pub(crate) fn new(mtm: MainThreadMarker, value: f64) -> Self {
        let native = NSStackView::new(mtm);
        native.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        native.setDistribution(NSStackViewDistribution::GravityAreas);
        native.setSpacing(2.0);

        let field = NSTextField::textFieldWithString(&NSString::from_str(""), mtm);
        // 数字は右そろえ。AppKit の数値欄と同じ見え方にする。
        field.setAlignment(NSTextAlignment::Right);

        let stepper = NSStepper::new(mtm);
        stepper.setValueWraps(false);
        stepper.setAutorepeat(true);
        // 幅の余りは欄が受け取り、上下のボタンは自分の大きさのままでいる
        // (欄は `NSTextField` の既定のまま = ボタンより弱く張り付く)。
        stepper.setContentHuggingPriority_forOrientation(
            STEPPER_HUGGING,
            NSLayoutConstraintOrientation::Horizontal,
        );

        let spec = NumberSpec::default();
        let this = Self(Rc::new(NumberInputInner {
            native,
            field,
            stepper,
            spec: Cell::new(spec),
            value: Cell::new(spec.clamp(value)),
            handler: ValueHandler::default(),
            silent: Cell::new(false),
            observer: RefCell::new(None),
            targets: RefCell::new(Vec::new()),
        }));

        this.0.native.addArrangedSubview(&this.0.field);
        this.0.native.addArrangedSubview(&this.0.stepper);
        let width = this
            .0
            .field
            .widthAnchor()
            .constraintGreaterThanOrEqualToConstant(FIELD_MIN_WIDTH);
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[width]));

        this.write_native_spec();
        this.write_native(this.value());
        this.connect(mtm);
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
        self.0.field.setEnabled(enabled);
        self.0.stepper.setEnabled(enabled);
    }

    /// 値が変わったときに、変わったあとの値で呼ばれる。
    /// 設定し直すと以前のコールバックは外れる。
    pub fn on_change(&self, f: impl FnMut(f64) + 'static) {
        self.0.handler.set(f);
    }

    /// 数字を打つ欄。バックエンド固有の脱出口として公開している。
    pub fn native_field(&self) -> Retained<NSTextField> {
        self.0.field.clone()
    }

    /// 上下のボタン。
    pub fn native_stepper(&self) -> Retained<NSStepper> {
        self.0.stepper.clone()
    }

    /// 打鍵・確定・上下のボタンの購読をつなぐ。
    fn connect(&self, mtm: MainThreadMarker) {
        // 打鍵のたびに、読める値なら受け取る。読めないもの (途中の `-` や
        // 空欄) は確定まで放っておく。
        let observer = TextObserver::new(mtm, {
            let weak = Rc::downgrade(&self.0);
            move |text: &str| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let this = NumberInput(inner);
                if this.0.silent.get() {
                    return;
                }
                if let Some(shown) = this.0.spec.get().parse(text) {
                    this.accept(shown, false);
                }
            }
        });
        unsafe {
            self.0
                .field
                .setDelegate(Some(ProtocolObject::from_ref(&*observer)))
        };
        *self.0.observer.borrow_mut() = Some(observer);

        // 確定 (Enter・欄を離れる)。読めなかった表示はここで元へ戻す。
        let commit = ActionTarget::new(mtm, {
            let weak = Rc::downgrade(&self.0);
            move || {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let this = NumberInput(inner);
                let text = this.0.field.stringValue().to_string();
                let shown = this
                    .0
                    .spec
                    .get()
                    .parse(&text)
                    .unwrap_or_else(|| this.value());
                this.accept(shown, true);
            }
        });
        unsafe {
            self.0.field.setTarget(Some(&commit));
            self.0.field.setAction(Some(sel!(invoke:)));
        }

        // 上下のボタン。刻みと範囲は `NSStepper` 自身が守る。
        let stepped = ActionTarget::new(mtm, {
            let weak = Rc::downgrade(&self.0);
            move || {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let this = NumberInput(inner);
                if this.0.silent.get() {
                    return;
                }
                let shown = this.0.stepper.doubleValue();
                this.accept(shown, true);
            }
        });
        unsafe {
            self.0.stepper.setTarget(Some(&stepped));
            self.0.stepper.setAction(Some(sel!(invoke:)));
        }
        *self.0.targets.borrow_mut() = vec![commit, stepped];
    }

    /// 決まりを差し替え、ネイティブと現在値へ反映する。
    fn update_spec(&self, edit: impl FnOnce(NumberSpec) -> NumberSpec) {
        self.0.spec.set(edit(self.0.spec.get()));
        self.write_native_spec();
        self.set_value(self.value());
    }

    /// 画面に出ている値を受け取る。`commit` なら表示も書き直す。
    fn accept(&self, shown: f64, commit: bool) {
        let accepted = self.0.spec.get().clamp(shown);
        if commit {
            self.write_native(accepted);
        } else {
            // 打っている最中でも、上下のボタンの位置だけは合わせておく。
            self.write_native_stepper(accepted);
        }
        if accepted == self.value() {
            return;
        }
        self.0.value.set(accepted);
        self.0.handler.emit(accepted);
    }

    /// 値を欄とボタンへ書く。この間の通知は無視する。
    fn write_native(&self, value: f64) {
        let previous = self.0.silent.replace(true);
        self.0
            .field
            .setStringValue(&NSString::from_str(&self.0.spec.get().format(value)));
        self.0.stepper.setDoubleValue(value);
        self.0.silent.set(previous);
    }

    fn write_native_stepper(&self, value: f64) {
        let previous = self.0.silent.replace(true);
        self.0.stepper.setDoubleValue(value);
        self.0.silent.set(previous);
    }

    /// 範囲と刻みを `NSStepper` へ渡す。
    fn write_native_spec(&self) {
        let spec = self.0.spec.get();
        self.0
            .stepper
            .setMinValue(spec.min.unwrap_or(-STEPPER_LIMIT));
        self.0
            .stepper
            .setMaxValue(spec.max.unwrap_or(STEPPER_LIMIT));
        self.0.stepper.setIncrement(spec.step);
    }
}
