//! Number input (`<input type="number">`).
//!
//! The browser provides the up/down buttons (spinner) and input that accepts only
//! digits. naui only maps [`NumberSpec`] onto the `min` / `max` / `step` attributes
//! and converts the value string to and from `f64`.
//!
//! Rewriting the display while the user is typing would make typing awkward, so
//! **it is written back only on confirmation (`change`)** (the same as
//! [`DatePicker`](crate::DatePicker)).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{NumberSpec, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlInputElement};

use crate::keys::KeyDown;
use crate::widgets::{create, impl_widget, Listener, ValueHandler, Widget};

struct NumberInputInner {
    input: HtmlInputElement,
    spec: Cell<NumberSpec>,
    value: Cell<f64>,
    on_change: ValueHandler<f64>,
    on_submit: ValueHandler<f64>,
    /// The subscriptions for input and for confirmation. Dropping them also
    /// unsubscribes.
    listeners: RefCell<Vec<Listener>>,
    key_down: KeyDown,
    focus: crate::focus::Focus,
}

/// A control for entering a number (`<input type="number">`).
///
/// The default is integers (step 1, 0 decimal places, no range limit).
#[derive(Clone)]
pub struct NumberInput(Rc<NumberInputInner>);
impl_widget!(NumberInput, input);
crate::keys::impl_key_down!(NumberInput);
crate::focus::impl_focus_change!(NumberInput);

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
            on_submit: ValueHandler::default(),
            listeners: RefCell::new(Vec::new()),
            key_down: KeyDown::default(),
            focus: Default::default(),
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
        // Enter。ブラウザによっては Enter で `change` が出ないので、ここで
        // 確定させてから知らせる (`change` が続いても同じ値を読むだけ)。
        let submit = crate::widgets::on_enter(&this.0.input, {
            let weak = Rc::downgrade(&this.0);
            move |_| {
                if let Some(inner) = weak.upgrade() {
                    let this = NumberInput(inner);
                    this.native_changed(true);
                    this.0.on_submit.emit(this.value());
                }
            }
        });
        listeners.extend(submit);
        *this.0.listeners.borrow_mut() = listeners;
        Ok(this)
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
        self.0.input.set_disabled(!enabled);
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

    /// The corresponding `<input>`. Public as a backend-specific escape hatch.
    pub fn native_input(&self) -> HtmlInputElement {
        self.0.input.clone()
    }

    /// Handles a value change on the browser side.
    ///
    /// `commit` is whether it is the `change` from leaving the field.
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

    /// Writes the value to the `<input>`. Assigning `value` fires neither `input` nor
    /// `change`, so the subscriptions need not be stopped.
    fn write_native(&self, value: f64) {
        self.0.input.set_value(&self.0.spec.get().format(value));
    }

    /// Writes the range and step into attributes. Passing an empty string removes the
    /// attribute, returning to no limit.
    fn write_native_spec(&self) {
        let spec = self.0.spec.get();
        let bound = |value: Option<f64>| value.map(|v| v.to_string()).unwrap_or_default();
        self.0.input.set_min(&bound(spec.min));
        self.0.input.set_max(&bound(spec.max));
        self.0.input.set_step(&spec.step.to_string());
    }
}
