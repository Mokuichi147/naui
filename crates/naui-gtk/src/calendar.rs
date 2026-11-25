//! An always-visible month calendar (`GtkCalendar`).
//!
//! `GtkCalendar` is the same control `DatePicker` shows in its popover, placed
//! directly. It is always Gregorian, and the first day of the week and the names
//! follow the locale.
//!
//! Two things differ from the other platforms, both from `GtkCalendar` itself:
//!
//! - **It has no range.** Days outside [`set_range`](Calendar::set_range) are not
//!   dimmed and can be clicked; naui moves the selection back to the nearest end
//!   (and notifies if that changed the value).
//! - **Moving to another month or year selects a day in it.** The arrows in the
//!   header move the selection along (keeping the day of the month), and the
//!   change is notified like a click.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use naui_core::{DatePickerMode, DateTime};

use crate::bin::SizeBin;
use crate::callback::Notifier;
use crate::date_picker::{now, to_glib_date_time};
use crate::widgets::{impl_widget, without_signal, Widget};

/// Only the date is picked, as in a `DatePicker` that shows only the date.
const MODE: DatePickerMode = DatePickerMode::Date;

struct CalendarInner {
    native: gtk::Calendar,
    bin: SizeBin,
    /// The `day-selected` handler, blocked while writing the value from code.
    handler: RefCell<Option<glib::SignalHandlerId>>,
    /// The current value, **including the time**, which the calendar does not show.
    value: Cell<DateTime>,
    min: Cell<Option<DateTime>>,
    max: Cell<Option<DateTime>>,
    on_change: Notifier<DateTime>,
}

/// An always-visible month calendar (`GtkCalendar`).
///
/// Its value right after creation is the platform's current date and time (local
/// time).
#[derive(Clone)]
pub struct Calendar(Rc<CalendarInner>);
impl_widget!(Calendar);

impl Calendar {
    pub(crate) fn new() -> Self {
        let native = gtk::Calendar::new();
        let bin = SizeBin::wrap(&native);
        let this = Self(Rc::new(CalendarInner {
            native,
            bin,
            handler: RefCell::new(None),
            value: Cell::new(now()),
            min: Cell::new(None),
            max: Cell::new(None),
            on_change: Notifier::default(),
        }));
        this.write_native(this.value());

        let changed = {
            let weak = Rc::downgrade(&this.0);
            move |_: &gtk::Calendar| {
                if let Some(inner) = weak.upgrade() {
                    Calendar(inner).native_changed();
                }
            }
        };
        let handler = this.0.native.connect_day_selected(changed.clone());
        *this.0.handler.borrow_mut() = Some(handler);
        // 見出しの矢印で月や年を送ると、GtkCalendar は選んでいる日もその月へ
        // 動かす。ところが GTK 4.24 で確かめると day-selected は出ず、
        // next-month などだけが出る。受け取らないと表示と値が食い違うので、
        // 4 つとも日を選んだのと同じく扱う (値が同じなら何もしない)。
        // コードから select_day するときはこれらは出ない。
        this.0.native.connect_next_month(changed.clone());
        this.0.native.connect_prev_month(changed.clone());
        this.0.native.connect_next_year(changed.clone());
        this.0.native.connect_prev_year(changed);
        this
    }

    /// The selected date. The time part is returned as it was passed.
    pub fn value(&self) -> DateTime {
        self.0.value.get()
    }

    /// Selects a date without notifying, and shows its month. Out-of-range dates are
    /// clamped to the ends, and values that are not valid dates are normalized.
    pub fn set_value(&self, value: DateTime) {
        let value = self.clamp(value);
        self.0.value.set(value);
        self.write_native(value);
    }

    /// Sets the selectable range of days. `None` places no limit on that side. Only
    /// the dates are compared; the times of `min` and `max` are ignored.
    ///
    /// If the current value falls outside the range, it is clamped to the end without
    /// notifying. `GtkCalendar` has no range, so the days outside it are not dimmed:
    /// clicking one moves the selection back to the nearest end.
    pub fn set_range(&self, min: Option<DateTime>, max: Option<DateTime>) {
        self.0.min.set(min.map(DateTime::normalized));
        self.0.max.set(max.map(DateTime::normalized));
        self.set_value(self.value());
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.0.native.set_sensitive(enabled);
    }

    /// Called with the new value when the user selects another day, including by
    /// moving to another month or year with the arrows in the header. Setting it
    /// again removes the previous callback.
    pub fn on_change(&self, f: impl FnMut(DateTime) + 'static) {
        self.0.on_change.set(f);
    }

    /// Selects the day of `value` the way the user does by clicking it: the
    /// calendar shows its month, the day is selected, and
    /// [`on_change`](Self::on_change) is called with the new value (the time part
    /// stays as it was). A date outside the range is clamped to the end first.
    /// Nothing is called when the day is already selected. Does nothing while
    /// disabled.
    ///
    /// The day is selected with `gtk_calendar_select_day`, whose `day-selected`
    /// reaches naui as a click's does. For tests and automation.
    pub fn pick(&self, value: DateTime) {
        if !self.0.native.is_sensitive() {
            return;
        }
        if let Ok(date) = to_glib_date_time(self.clamp(value)) {
            self.0.native.select_day(&date);
        }
    }

    /// The real `GtkCalendar`. Public as a backend-specific escape hatch.
    pub fn native_calendar(&self) -> gtk::Calendar {
        self.0.native.clone()
    }

    /// Handles a change of the selected day on the native side.
    fn native_changed(&self) {
        let date = self.0.native.date();
        let shown = DateTime::date(date.year(), date.month() as u8, date.day_of_month() as u8);
        let accepted = self.clamp(MODE.apply(self.value(), shown));
        // 範囲で押し戻したときのために、表示は必ず書き直す
        // (同じ日なら GTK 側は何もしない)。
        self.write_native(accepted);
        if accepted == self.value() {
            return;
        }
        self.0.value.set(accepted);
        self.0.on_change.emit(accepted);
    }

    fn clamp(&self, value: DateTime) -> DateTime {
        MODE.clamp(value, self.0.min.get(), self.0.max.get())
    }

    /// Selects the day on the calendar. `day-selected` is blocked meanwhile.
    fn write_native(&self, value: DateTime) {
        let Ok(date) = to_glib_date_time(value) else {
            return;
        };
        without_signal(&self.0.native, &self.0.handler, || {
            self.0.native.select_day(&date);
        });
    }
}
