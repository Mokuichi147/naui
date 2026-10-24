//! Number input (`GtkSpinButton`).
//!
//! GTK4 has a dedicated numeric control, so it is enough to map [`NumberSpec`] onto
//! the range (`GtkAdjustment`), step and digits.
//!
//! Rounding and range are also kept on naui's side. `GtkSpinButton` rounds only the
//! display to the digits and does not round the value itself, so to return the same
//! answer on all four backends, the value passed through [`NumberSpec`] must be
//! kept.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use naui_core::NumberSpec;

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::widgets::{impl_widget, without_signal, Widget};

/// The bound passed to `GtkAdjustment` when no range is specified.
///
/// `GtkSpinButton` always has a minimum and maximum, so a magnitude unreachable in
/// practice is used in place of "no limit". [`NumberSpec`] holds the rounding and
/// range, so stopping here does not change naui's answer.
const RANGE_LIMIT: f64 = 1.0e15;

/// The number of digits the field is sized to show.
///
/// When deciding its width, `GtkSpinButton` counts the digits of numbers within the
/// range. A field without a range would want the 16 digits of [`RANGE_LIMIT`], so
/// the "natural size" is decided again here. The minimum allows the field down to
/// 0 digits (`set_width_chars(0)`) and leaves the width to the app.
const FIELD_CHARS: i32 = 8;

struct NumberInputInner {
    native: gtk::SpinButton,
    bin: SizeBin,
    spec: Cell<NumberSpec>,
    value: Cell<f64>,
    on_change: Notifier<f64>,
    on_submit: Notifier<f64>,
    /// The signal that is blocked while writing the value from code.
    value_handler: RefCell<Option<glib::SignalHandlerId>>,
    text_handler: RefCell<Option<glib::SignalHandlerId>>,
}

/// A control for entering a number (`GtkSpinButton`).
///
/// The default is integers (step 1, 0 decimal places, no range limit).
#[derive(Clone)]
pub struct NumberInput(Rc<NumberInputInner>);
crate::keys::impl_key_down!(NumberInput);
crate::focus::impl_focus_change!(NumberInput);
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
            on_submit: Notifier::default(),
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
        this.connect_submit();
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
        self.0.native.set_sensitive(enabled);
    }

    /// Called with the new value when the value changes. Setting it again removes the
    /// previous callback.
    pub fn on_change(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_change.set(f);
    }

    /// Called with the confirmed value when the user confirms with Enter.
    ///
    /// What has been typed so far is read into the value first (if the value changes,
    /// `on_change` is called first). Text that cannot be read as a number is reverted
    /// to the previous value before the call. Not called for an Enter pressed while an
    /// IME is composing.
    pub fn on_submit(&self, f: impl FnMut(f64) + 'static) {
        self.0.on_submit.set(f);
    }

    /// The real GTK4 control. Public as a backend-specific escape hatch.
    pub fn native_spin_button(&self) -> gtk::SpinButton {
        self.0.native.clone()
    }

    /// Connects Enter confirmation.
    ///
    /// `GtkSpinButton`'s own `activate` signal exists only since GTK 4.14, so the
    /// `activate` of the inner `GtkText` (the delegate of `GtkEditable`) is received.
    /// `GtkSpinButton` connects value confirmation (`gtk_spin_button_update`) to the
    /// same signal at creation, so by the time this later-connected handler is called,
    /// the value is settled (unreadable text has been reverted to the previous value).
    fn connect_submit(&self) {
        let Some(text) = self.0.native.delegate() else {
            return;
        };
        let Ok(text) = text.downcast::<gtk::Text>() else {
            return;
        };
        let weak = Rc::downgrade(&self.0);
        text.connect_activate(move |_| {
            if let Some(inner) = weak.upgrade() {
                let this = NumberInput(inner);
                this.0.on_submit.emit(this.value());
            }
        });
    }

    /// Receives the value on screen. With `commit`, rewrites the display too.
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

    /// Writes the value into the control. Signals are blocked meanwhile.
    fn write_native(&self, value: f64) {
        without_signal(&self.0.native, &self.0.value_handler, || {
            without_signal(&self.0.native, &self.0.text_handler, || {
                self.0.native.set_value(value);
            });
        });
    }

    /// Passes the range, step and digits to the `GtkSpinButton`.
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
