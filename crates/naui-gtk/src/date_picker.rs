//! Date and time picking (`GtkMenuButton` + `GtkCalendar` / `GtkSpinButton`).
//!
//! GTK4 has no single control for picking a date. It does have `GtkCalendar` for
//! calendars and `GtkSpinButton` for numbers, so they are combined the way GNOME
//! apps do.
//!
//! - Date: clicking a `GtkMenuButton` showing the current date opens a
//!   `GtkCalendar` inside a `GtkPopover`.
//! - Time: `GtkSpinButton`s for hours and minutes, separated by `:` (the same shape
//!   as setting an alarm in the Clocks app).
//!
//! **Picking a date does not close the popover.** `GtkCalendar` does not
//! distinguish "clicked a day" from "moved to another month"; both arrive as
//! `day-selected`. Closing on every one would close it just for changing months.
//!
//! The date shown on the button follows the locale's format (`%x`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use naui_core::{DatePickerMode, DateTime};

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::widgets::{impl_widget, without_signal, Widget};

struct DatePickerInner {
    native: gtk::Box,
    bin: SizeBin,
    mode: DatePickerMode,
    /// The button showing the date, and the calendar inside it. When only the time can
    /// be picked, they are created but not laid out.
    button: gtk::MenuButton,
    calendar: gtk::Calendar,
    hour: gtk::SpinButton,
    minute: gtk::SpinButton,
    /// The signal that is blocked while writing the value from code.
    calendar_handler: RefCell<Option<glib::SignalHandlerId>>,
    hour_handler: RefCell<Option<glib::SignalHandlerId>>,
    minute_handler: RefCell<Option<glib::SignalHandlerId>>,
    /// The current value, **including the parts that cannot be picked**.
    value: Cell<DateTime>,
    min: Cell<Option<DateTime>>,
    max: Cell<Option<DateTime>>,
    on_change: Notifier<DateTime>,
}

/// A control for picking a date and time.
///
/// Its value right after creation is the platform's current date and time (local
/// time).
#[derive(Clone)]
pub struct DatePicker(Rc<DatePickerInner>);
impl_widget!(DatePicker);

impl DatePicker {
    pub(crate) fn new(mode: DatePickerMode) -> Self {
        let native = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let bin = SizeBin::wrap(&native);

        let calendar = gtk::Calendar::new();
        let popover = gtk::Popover::new();
        popover.set_child(Some(&calendar));
        let button = gtk::MenuButton::new();
        button.set_popover(Some(&popover));

        let hour = spin(0.0, 23.0);
        let minute = spin(0.0, 59.0);

        if mode.has_date() {
            native.append(&button);
        }
        if mode.has_time() {
            let separator = gtk::Label::new(Some(":"));
            native.append(&hour);
            native.append(&separator);
            native.append(&minute);
        }

        let inner = Rc::new(DatePickerInner {
            native,
            bin,
            mode,
            button,
            calendar,
            hour,
            minute,
            calendar_handler: RefCell::new(None),
            hour_handler: RefCell::new(None),
            minute_handler: RefCell::new(None),
            value: Cell::new(now()),
            min: Cell::new(None),
            max: Cell::new(None),
            on_change: Notifier::default(),
        });
        let this = Self(inner);
        this.write_native(this.value());

        // 3 つのコントロールは、どれも「表示されている値が変わった」を
        // 伝えるだけ。値の組み立ては read_native がまとめて行う。
        let calendar_handler = {
            let weak = Rc::downgrade(&this.0);
            this.0.calendar.connect_day_selected(move |_| {
                if let Some(inner) = weak.upgrade() {
                    DatePicker(inner).native_changed();
                }
            })
        };
        *this.0.calendar_handler.borrow_mut() = Some(calendar_handler);
        for (spin, slot) in [
            (&this.0.hour, &this.0.hour_handler),
            (&this.0.minute, &this.0.minute_handler),
        ] {
            let handler = {
                let weak = Rc::downgrade(&this.0);
                spin.connect_value_changed(move |_| {
                    if let Some(inner) = weak.upgrade() {
                        DatePicker(inner).native_changed();
                    }
                })
            };
            *slot.borrow_mut() = Some(handler);
        }
        this
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
    /// notifying. Neither `GtkCalendar` nor `GtkSpinButton` has a "calendar range", so
    /// the range is enforced only by naui's clamping.
    pub fn set_range(&self, min: Option<DateTime>, max: Option<DateTime>) {
        self.0.min.set(min.map(DateTime::normalized));
        self.0.max.set(max.map(DateTime::normalized));
        self.set_value(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// Called with the new value when the value changes. Setting it again removes the
    /// previous callback.
    pub fn on_change(&self, f: impl FnMut(DateTime) + 'static) {
        self.0.on_change.set(f);
    }

    /// The button showing the date. Public as a backend-specific escape hatch.
    pub fn native_button(&self) -> gtk::MenuButton {
        self.0.button.clone()
    }

    /// The calendar inside the popover.
    pub fn native_calendar(&self) -> gtk::Calendar {
        self.0.calendar.clone()
    }

    /// The `GtkSpinButton`s for hours and minutes.
    pub fn native_spins(&self) -> (gtk::SpinButton, gtk::SpinButton) {
        (self.0.hour.clone(), self.0.minute.clone())
    }

    /// Handles a change in what any of the controls shows.
    fn native_changed(&self) {
        let shown = self.read_native();
        let accepted = self.clamp(self.0.mode.apply(self.value(), shown));
        // 丸めや範囲で押し戻したときのために、表示は必ず書き直す
        // (同じ値なら GTK 側は何もしない)。
        self.write_native(accepted);
        if accepted == self.value() {
            return;
        }
        self.0.value.set(accepted);
        self.0.on_change.emit(accepted);
    }

    /// The year, month, day, hour and minute currently on screen.
    fn read_native(&self) -> DateTime {
        let date = self.0.calendar.date();
        DateTime {
            year: date.year(),
            month: date.month() as u8,
            day: date.day_of_month() as u8,
            hour: self.0.hour.value() as u8,
            minute: self.0.minute.value() as u8,
        }
        .normalized()
    }

    fn clamp(&self, value: DateTime) -> DateTime {
        self.0.mode.clamp(value, self.0.min.get(), self.0.max.get())
    }

    /// Writes the value into the three controls. Signals are blocked meanwhile.
    fn write_native(&self, value: DateTime) {
        if let Ok(date) = to_glib_date_time(value) {
            without_signal(&self.0.calendar, &self.0.calendar_handler, || {
                self.0.calendar.select_day(&date);
            });
            self.0.button.set_label(&format_date(&date));
        }
        without_signal(&self.0.hour, &self.0.hour_handler, || {
            self.0.hour.set_value(value.hour as f64);
        });
        without_signal(&self.0.minute, &self.0.minute_handler, || {
            self.0.minute.set_value(value.minute as f64);
        });
    }
}

/// A zero-based spin button showing two digits. `TimePicker` uses the same shape
/// for its time fields, so it is used from there too.
pub(crate) fn spin(min: f64, max: f64) -> gtk::SpinButton {
    let spin = gtk::SpinButton::with_range(min, max, 1.0);
    spin.set_numeric(true);
    // 23 の次は 0 へ戻る。時刻の入力では端で止まるより自然。
    spin.set_wrap(true);
    spin.set_width_chars(2);
    spin.set_max_width_chars(2);
    // 既定の表示は `9` になるので、`09` へそろえる。
    spin.connect_output(|spin| {
        spin.set_text(&format!("{:02}", spin.value() as i32));
        glib::Propagation::Stop
    });
    spin
}

/// The locale's date format (`%x`). Falls back to ISO form if it cannot be
/// obtained.
fn format_date(date: &glib::DateTime) -> String {
    date.format("%x")
        .map(|text| text.to_string())
        .unwrap_or_else(|_| {
            format!(
                "{:04}-{:02}-{:02}",
                date.year(),
                date.month(),
                date.day_of_month()
            )
        })
}

fn to_glib_date_time(value: DateTime) -> Result<glib::DateTime, glib::BoolError> {
    let v = value.normalized();
    glib::DateTime::from_local(
        v.year,
        v.month as i32,
        v.day as i32,
        v.hour as i32,
        v.minute as i32,
        0.0,
    )
}

/// The current date and time on the GTK side (local time).
fn now() -> DateTime {
    let Ok(now) = glib::DateTime::now_local() else {
        return DateTime::default();
    };
    DateTime {
        year: now.year(),
        month: now.month() as u8,
        day: now.day_of_month() as u8,
        hour: now.hour() as u8,
        minute: now.minute() as u8,
    }
    .normalized()
}
