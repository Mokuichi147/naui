//! Number input (`NSTextField` + `NSStepper`).
//!
//! AppKit has no dedicated numeric control. The standard AppKit arrangement is a
//! field for typing digits next to up/down buttons (the same shape as numeric
//! fields in System Settings), so an `NSTextField` and an `NSStepper` are laid out
//! in an `NSStackView`.
//!
//! Rounding and range are decided by [`NumberSpec`]. Rewriting the display while the
//! user is typing would make typing awkward, so **it is written back only on
//! confirmation (Enter, or leaving the field) and when the up/down buttons are
//! pressed**.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::NumberSpec;
use objc2::rc::Retained;
use objc2::{sel, MainThreadMarker, Message};
use objc2_app_kit::{
    NSLayoutConstraint, NSLayoutConstraintOrientation, NSLayoutPriority, NSStackView,
    NSStackViewDistribution, NSStepper, NSTextAlignment, NSTextField,
    NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::{NSArray, NSString};

use crate::trampoline::{ActionTarget, FieldHandlers, FieldObserver, ValueHandler};
use crate::widgets::{impl_widget, observe_field, Widget};

/// The minimum width of the digit field (logical pixels). Fitting the content would
/// shrink it to one digit.
const FIELD_MIN_WIDTH: f64 = 72.0;

/// The bound passed to `NSStepper` when no range is specified.
///
/// `NSStepper` always has a minimum and maximum, so a magnitude unreachable in
/// practice is used in place of "no limit". [`NumberSpec`] holds the rounding and
/// range, so stopping here does not change naui's answer.
const STEPPER_LIMIT: f64 = 1.0e15;

/// The up/down buttons keep their own size and hand leftover width to the field.
const STEPPER_HUGGING: NSLayoutPriority = 1000.0;

struct NumberInputInner {
    native: Retained<NSStackView>,
    field: Retained<NSTextField>,
    stepper: Retained<NSStepper>,
    spec: Cell<NumberSpec>,
    value: Cell<f64>,
    handler: ValueHandler<f64>,
    submit: ValueHandler<f64>,
    /// Ignores notifications from the native side only while writing the value.
    silent: Cell<bool>,
    /// The handlers for typing and Enter in the field.
    field_handlers: Rc<FieldHandlers>,
    /// AppKit targets and delegates are weak, so they are kept alive here.
    _observer: Retained<FieldObserver>,
    targets: RefCell<Vec<Retained<ActionTarget>>>,
}

/// A control for entering a number (`NSTextField` + `NSStepper`).
///
/// The default is integers (step 1, 0 decimal places, no range limit).
#[derive(Clone)]
pub struct NumberInput(Rc<NumberInputInner>);
crate::keys::impl_key_down!(NumberInput);
crate::focus::impl_focus_change!(NumberInput);
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
        let (field_handlers, observer) = observe_field(mtm, &field);
        let this = Self(Rc::new(NumberInputInner {
            native,
            field,
            stepper,
            spec: Cell::new(spec),
            value: Cell::new(spec.clamp(value)),
            handler: ValueHandler::default(),
            submit: ValueHandler::default(),
            silent: Cell::new(false),
            field_handlers,
            _observer: observer,
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

    /// The current value.
    pub fn value(&self) -> f64 {
        self.0.value.get()
    }

    /// Changes the value without notifying. Rounds to the decimal places and clamps to
    /// the range.
    pub fn set_value(&self, value: f64) {
        let value = self.0.spec.get().clamp(value);
        self.0.value.set(value);
        self.write_native(value);
    }

    /// Sets the range of accepted values. `None` places no limit on that side.
    ///
    /// If the current value falls outside the range, it is clamped to the end without
    /// notifying.
    pub fn set_range(&self, min: Option<f64>, max: Option<f64>) {
        self.update_spec(|spec| spec.range(min, max));
    }

    /// How much one press of the up/down buttons or keys moves the value (default 1).
    pub fn set_step(&self, step: f64) {
        self.update_spec(|spec| spec.step(step));
    }

    /// The number of decimal places shown (default 0 = integers).
    pub fn set_decimals(&self, decimals: u32) {
        self.update_spec(|spec| spec.decimals(decimals));
    }

    /// The current value rules (range, step, decimal places).
    pub fn spec(&self) -> NumberSpec {
        self.0.spec.get()
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.field.setEnabled(enabled);
        self.0.stepper.setEnabled(enabled);
    }

    /// Called with the new value when the value changes. Setting it again removes the
    /// previous callback.
    pub fn on_change(&self, f: impl FnMut(f64) + 'static) {
        self.0.handler.set(f);
    }

    /// Called with the confirmed value when the user confirms with Enter.
    ///
    /// What has been typed so far is read into the value first (if the value changes,
    /// `on_change` is called first). Text that cannot be read as a number is reverted
    /// to the previous value before the call.
    pub fn on_submit(&self, f: impl FnMut(f64) + 'static) {
        self.0.submit.set(f);
    }

    /// The field for typing digits. Public as a backend-specific escape hatch.
    pub fn native_field(&self) -> Retained<NSTextField> {
        self.0.field.clone()
    }

    /// The up/down buttons.
    pub fn native_stepper(&self) -> Retained<NSStepper> {
        self.0.stepper.clone()
    }

    /// Connects the subscriptions for typing, confirmation and the up/down buttons.
    fn connect(&self, mtm: MainThreadMarker) {
        // 打鍵のたびに、読める値なら受け取る。読めないもの (途中の `-` や
        // 空欄) は確定まで放っておく。
        self.0.field_handlers.set_change({
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

        // Enter。AppKit の確定 (下の action) より先に来るので、ここで値を
        // 確定させてから知らせる。続く action は同じ値を読むだけになる。
        self.0.field_handlers.set_submit({
            let weak = Rc::downgrade(&self.0);
            move |_text: &str| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let this = NumberInput(inner);
                this.commit_field();
                this.0.submit.emit(this.value());
            }
        });

        // 確定 (Enter・欄を離れる)。読めなかった表示はここで元へ戻す。
        let commit = ActionTarget::new(mtm, {
            let weak = Rc::downgrade(&self.0);
            move || {
                if let Some(inner) = weak.upgrade() {
                    NumberInput(inner).commit_field();
                }
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

    /// Confirms the field's text as the value. Unreadable text is reverted to the
    /// previous value.
    fn commit_field(&self) {
        let text = self.0.field.stringValue().to_string();
        let shown = self
            .0
            .spec
            .get()
            .parse(&text)
            .unwrap_or_else(|| self.value());
        self.accept(shown, true);
    }

    /// Replaces the rules and applies them to the native side and the current value.
    fn update_spec(&self, edit: impl FnOnce(NumberSpec) -> NumberSpec) {
        self.0.spec.set(edit(self.0.spec.get()));
        self.write_native_spec();
        self.set_value(self.value());
    }

    /// Receives the value on screen. With `commit`, rewrites the display too.
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

    /// Writes the value into the field and buttons. Notifications during this are
    /// ignored.
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

    /// Passes the range and step to the `NSStepper`.
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
