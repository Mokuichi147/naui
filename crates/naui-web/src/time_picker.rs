//! Time picking (`<input type="time">`).
//!
//! The browser shows the time input UI (spinners or a clock popup). naui only
//! converts the `value`'s `"09:30"` to and from [`Time`]; there is nothing it builds
//! itself. Whether it is 12- or 24-hour follows the browser's locale.
//!
//! Seconds are not handled. `<input type="time">` returns only minutes unless `step`
//! is made finer, matching the range [`Time`] holds.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{Result, Time};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlInputElement};

use crate::widgets::{create, impl_widget, Listener, ValueHandler, Widget};

struct TimePickerInner {
    input: HtmlInputElement,
    value: Cell<Time>,
    min: Cell<Option<Time>>,
    max: Cell<Option<Time>>,
    on_change: ValueHandler<Time>,
    /// The subscriptions for input and for confirmation. Dropping them also
    /// unsubscribes.
    listeners: RefCell<Vec<Listener>>,
}

/// A control for picking a time of day (`<input type="time">`).
///
/// Its value right after creation is the browser's current time (local time).
#[derive(Clone)]
pub struct TimePicker(Rc<TimePickerInner>);
impl_widget!(TimePicker, input);

impl TimePicker {
    pub(crate) fn new(document: &Document) -> Result<Self> {
        let input: HtmlInputElement = create(document, "input")?.unchecked_into();
        input.set_type("time");

        let this = Self(Rc::new(TimePickerInner {
            input,
            value: Cell::new(now()),
            min: Cell::new(None),
            max: Cell::new(None),
            on_change: ValueHandler::default(),
            listeners: RefCell::new(Vec::new()),
        }));
        this.write_native(this.value());

        // 打鍵やスピナーの操作は `input`、欄を離れたときの確定は `change`。
        // 途中の状態 (`09:` など) では値が空文字で届くので、読めたものだけ
        // 受け取り、確定時には読めなかった表示を元の値へ戻す
        // (`DatePicker` と同じ扱い)。
        let mut listeners = Vec::new();
        for event in ["input", "change"] {
            let listener = Listener::attach(this.0.input.as_ref(), event, {
                let weak = Rc::downgrade(&this.0);
                let commit = event == "change";
                move || {
                    if let Some(inner) = weak.upgrade() {
                        TimePicker(inner).native_changed(commit);
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

    /// The currently selected time.
    pub fn value(&self) -> Time {
        self.0.value.get()
    }

    /// Changes the value without notifying. Out-of-range values are clamped to the
    /// ends, and values that are not valid times are normalized.
    pub fn set_value(&self, value: Time) {
        let value = self.clamp(value);
        self.0.value.set(value);
        self.write_native(value);
    }

    /// Sets the selectable range. `None` places no limit on that side.
    ///
    /// If the current value falls outside the range, it is clamped to the end without
    /// notifying.
    pub fn set_range(&self, min: Option<Time>, max: Option<Time>) {
        self.0.min.set(min.map(Time::normalized));
        self.0.max.set(max.map(Time::normalized));
        let format = |value: Option<Time>| value.map(|v| v.to_string()).unwrap_or_default();
        // 空文字を渡すと属性が外れ、制限なしに戻る。
        self.0.input.set_min(&format(self.0.min.get()));
        self.0.input.set_max(&format(self.0.max.get()));
        self.set_value(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.input.set_disabled(!enabled);
    }

    /// Called with the new value when the value changes. Setting it again removes the
    /// previous callback.
    pub fn on_change(&self, f: impl FnMut(Time) + 'static) {
        self.0.on_change.set(f);
    }

    /// The input itself. Public as a backend-specific escape hatch.
    pub fn native_input(&self) -> HtmlInputElement {
        self.0.input.clone()
    }

    /// Handles a value change on the browser side.
    ///
    /// `commit` is whether it is the `change` from leaving the field. Rewriting the
    /// display mid-input would make typing awkward, so writing back happens only on
    /// confirmation.
    fn native_changed(&self, commit: bool) {
        let text = self.0.input.value();
        let Some(shown) = Time::parse(&text) else {
            // 読めない (空欄を含む)。確定なら元の値へ戻す。
            if commit {
                self.write_native(self.value());
            }
            return;
        };
        let accepted = self.clamp(shown);
        if accepted.to_string() != text {
            // 範囲で押し戻したときは表示も直す。
            self.write_native(accepted);
        }
        if accepted == self.value() {
            return;
        }
        self.0.value.set(accepted);
        self.0.on_change.emit(accepted);
    }

    fn clamp(&self, value: Time) -> Time {
        value.clamped(self.0.min.get(), self.0.max.get())
    }

    /// Writes the value to the `<input>`. Assigning `value` fires neither `input` nor
    /// `change`, so the subscriptions need not be stopped.
    fn write_native(&self, value: Time) {
        self.0.input.set_value(&value.to_string());
    }
}

/// The browser's current time (local time).
fn now() -> Time {
    let now = js_sys::Date::new_0();
    Time::new(now.get_hours() as u8, now.get_minutes() as u8).normalized()
}
