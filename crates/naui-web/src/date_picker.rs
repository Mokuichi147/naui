//! Date and time picking (`<input type="date">` / `"time"` / `"datetime-local"`).
//!
//! The browser shows the calendar and time input UI. naui only decides the `type`
//! from [`DatePickerMode`] and converts the value string to and from [`DateTime`].
//!
//! `datetime-local` is used to keep the browser from doing time zone conversion.
//! naui's [`DateTime`] is "the value as the calendar on screen shows it" and has no
//! offset from UTC.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use naui_core::{DatePickerMode, DateTime, Result};
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlInputElement};

use crate::widgets::{create, impl_widget, Listener, ValueHandler, Widget};

struct DatePickerInner {
    input: HtmlInputElement,
    mode: DatePickerMode,
    /// The current value, **including the parts that cannot be picked**.
    /// `<input type="date">` has no time, so when only the date can be picked, the time
    /// survives only here.
    value: Cell<DateTime>,
    min: Cell<Option<DateTime>>,
    max: Cell<Option<DateTime>>,
    on_change: ValueHandler<DateTime>,
    /// The subscriptions for input and for confirmation. Dropping them also
    /// unsubscribes.
    listeners: RefCell<Vec<Listener>>,
    /// `on_file_drop`.
    file_drop: crate::file_drop::FileDrop,
}

/// A control for picking a date and time (the date-type `<input>`s).
///
/// Its value right after creation is the browser's current date and time (local
/// time).
#[derive(Clone)]
pub struct DatePicker(Rc<DatePickerInner>);
impl_widget!(DatePicker, input);

impl DatePicker {
    pub(crate) fn new(document: &Document, mode: DatePickerMode) -> Result<Self> {
        let input: HtmlInputElement = create(document, "input")?.unchecked_into();
        input.set_type(input_type(mode));

        let this = Self(Rc::new(DatePickerInner {
            input,
            mode,
            value: Cell::new(now()),
            min: Cell::new(None),
            max: Cell::new(None),
            on_change: ValueHandler::default(),
            listeners: RefCell::new(Vec::new()),
            file_drop: Default::default(),
        }));
        this.write_native(this.value());

        // 打鍵やカレンダーの選択は `input`、欄を離れたときの確定は `change`。
        // 途中の状態 (`2026-0` など) では値が空文字で届くので、読めたものだけ
        // 受け取り、確定時には読めなかった表示を元の値へ戻す。
        let mut listeners = Vec::new();
        for event in ["input", "change"] {
            let listener = Listener::attach(this.0.input.as_ref(), event, {
                let weak = Rc::downgrade(&this.0);
                let commit = event == "change";
                move || {
                    if let Some(inner) = weak.upgrade() {
                        DatePicker(inner).native_changed(commit);
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

    /// What it lets the user pick. Fixed at creation and never changes.
    pub fn mode(&self) -> DatePickerMode {
        self.0.mode
    }

    /// The current value. Parts that cannot be picked are returned as they were passed.
    pub fn value(&self) -> DateTime {
        self.0.value.get()
    }

    /// Changes the value without notifying. Out-of-range values are clamped to the
    /// ends, and values that are not valid dates are normalized.
    pub fn set_value(&self, value: DateTime) {
        let value = self.clamp(value);
        self.0.value.set(value);
        self.write_native(value);
    }

    /// Sets the selectable range. `None` places no limit on that side.
    ///
    /// If the current value falls outside the range, it is clamped to the end without
    /// notifying.
    pub fn set_range(&self, min: Option<DateTime>, max: Option<DateTime>) {
        self.0.min.set(min.map(DateTime::normalized));
        self.0.max.set(max.map(DateTime::normalized));
        let format =
            |value: Option<DateTime>| value.map(|v| self.0.mode.format(v)).unwrap_or_default();
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
    pub fn on_change(&self, f: impl FnMut(DateTime) + 'static) {
        self.0.on_change.set(f);
    }

    /// The corresponding `<input>`. Public as a backend-specific escape hatch.
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
        let Some(shown) = self.0.mode.parse(&text, self.value()) else {
            // 読めない (空欄を含む)。確定なら元の値へ戻す。
            if commit {
                self.write_native(self.value());
            }
            return;
        };
        let accepted = self.clamp(shown);
        if self.0.mode.format(accepted) != text {
            // 範囲で押し戻したときは表示も直す。
            self.write_native(accepted);
        }
        if accepted == self.value() {
            return;
        }
        self.0.value.set(accepted);
        self.0.on_change.emit(accepted);
    }

    fn clamp(&self, value: DateTime) -> DateTime {
        self.0.mode.clamp(value, self.0.min.get(), self.0.max.get())
    }

    /// Writes the value to the `<input>`. Assigning `value` fires neither `input` nor
    /// `change`, so the subscriptions need not be stopped.
    fn write_native(&self, value: DateTime) {
        self.0.input.set_value(&self.0.mode.format(value));
    }
}

fn input_type(mode: DatePickerMode) -> &'static str {
    match mode {
        DatePickerMode::Date => "date",
        DatePickerMode::Time => "time",
        DatePickerMode::DateTime => "datetime-local",
    }
}

/// The browser's current date and time (local time).
fn now() -> DateTime {
    let now = js_sys::Date::new_0();
    DateTime {
        year: now.get_full_year() as i32,
        // JavaScript の月は 0 始まり。
        month: (now.get_month() + 1) as u8,
        day: now.get_date() as u8,
        hour: now.get_hours() as u8,
        minute: now.get_minutes() as u8,
    }
    .normalized()
}
